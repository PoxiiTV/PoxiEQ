//! Procesos aparte para los plugins del usuario (`poxieq.exe --plugin-host …`). Cargan código ajeno: si un plugin
//! se cuelga o revienta, cae ese proceso y no PoxiEQ ni el audio de Windows.
//!
//! - `scan <ruta>`: escribe en la salida estándar un JSON con lo que trae el archivo (o `{"error": …}`).
//! - `serve <archivo.json>`: aloja de verdad el plugin del hueco descrito (el archivo se borra al leerlo). Un hilo
//!   de audio atiende el puente con el motor (`poxidsp::bridge`) y su ventana es la del plugin que suena. Por la
//!   entrada estándar llega `SHOW` (enseñar la ventana) y `QUIT`; por la salida va `STATE <base64>` cuando cambia
//!   su estado (cada segundo como mucho), `EDITOR 1`/`EDITOR 0` al abrir y cerrar la ventana y `ERROR …`.

use serde::{Deserialize, Serialize};
use poxidsp::bridge::{Server, MAX_BLOCK};
use poxidsp::host::Plugin;
use std::cell::UnsafeCell;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Un plugin encontrado al escanear
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub format: crate::eq::PluginFormat,
    pub path: String,
    pub cid: String,
    pub name: String,
    pub vendor: String,
    pub latency: u32,
}

/// Lo que necesita el proceso que aloja un plugin
#[derive(Serialize, Deserialize)]
pub struct HostJob {
    pub slot: crate::eq::PluginSlot,
    pub title: String,
    /// Clave del puente de audio con el motor (`PluginSlot::bridge_key`)
    pub bridge: u32,
}

pub fn main(args: &[String]) -> i32 {
    match args {
        [cmd, path] if cmd == "scan" => scan(path),
        [cmd, file] if cmd == "serve" => serve(file),
        _ => 2,
    }
}

fn out(line: &str) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{line}");
    let _ = o.flush();
}

fn hex(b: &[u8; 16]) -> String {
    if b.iter().all(|&x| x == 0) {
        return String::new();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn scan(path: &str) -> i32 {
    match poxidsp::host::scan(path) {
        Ok(list) => {
            let found: Vec<Found> = list
                .into_iter()
                .map(|i| Found {
                    format: if i.format == poxidsp::host::Format::Vst2 { crate::eq::PluginFormat::Vst2 } else { crate::eq::PluginFormat::Vst3 },
                    path: i.path,
                    cid: hex(&i.cid),
                    name: i.name,
                    vendor: i.vendor,
                    latency: i.latency,
                })
                .collect();
            out(&serde_json::to_string(&found).unwrap_or_default());
            0
        }
        Err(e) => {
            out(&serde_json::json!({ "error": e }).to_string());
            1
        }
    }
}

/// El plugin que suena, compartido entre el hilo de audio (solo `process`) y el de su ventana (lo demás), como en
/// cualquier host de VST: el plugin está hecho para eso. El cerrojo separa el proceso de lo que también procesa
/// o lo reconfigura (`flush`, `set_rate`).
struct Live {
    plugin: UnsafeCell<Plugin>,
    audio: Mutex<()>,
}

unsafe impl Sync for Live {}
unsafe impl Send for Live {}

impl Live {
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut Plugin {
        unsafe { &mut *self.plugin.get() }
    }
}

const WM_SHOW: u32 = WM_APP + 1;
const WM_QUIT_HOST: u32 = WM_APP + 2;
const WM_HIDE: u32 = WM_APP + 3;

fn serve(file: &str) -> i32 {
    let job: Option<HostJob> = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok());
    let _ = std::fs::remove_file(file);
    let Some(job) = job else { return 2 };
    let Some(spec) = job.slot.spec() else { return 2 };
    // La frecuencia del motor si el puente ya está; si no, 48 kHz (se ajusta con el primer bloque)
    let first = Server::open(job.bridge);
    let rate = first.as_ref().map(|s| s.rate()).filter(|&r| r >= 8000.0).unwrap_or(48000.0);
    let plugin = match Plugin::load_live(&spec, rate) {
        Ok(p) => p,
        Err(e) => {
            out(&format!("ERROR {e}"));
            return 1;
        }
    };
    let live = Arc::new(Live { plugin: UnsafeCell::new(plugin), audio: Mutex::new(()) });
    let stop = Arc::new(AtomicBool::new(false));
    let audio = {
        let (live, stop, key) = (live.clone(), stop.clone(), job.bridge);
        std::thread::spawn(move || audio_thread(&live, &stop, key, first, rate))
    };

    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return 1 };
        let inst = HINSTANCE(module.0);
        let class = w!("PoxiEQPlugin");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            lpszClassName: class,
            // El icono de la app (recurso 32512 que deja tauri-build); si no está, el de Windows
            hIcon: LoadIconW(Some(inst), PCWSTR(32512 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let title = HSTRING::from(job.title.as_str());
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE::default(), class, &title, style, CW_USEDEFAULT, CW_USEDEFAULT, 400, 300, None, None, Some(inst), None) else {
            return 1;
        };
        // Órdenes de la app por la entrada estándar; si la app se cierra (fin de la entrada), este proceso también
        let raw = hwnd.0 as isize;
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                let msg = match line.trim() {
                    "SHOW" => WM_SHOW,
                    "QUIT" => break,
                    _ => continue,
                };
                let _ = PostMessageW(Some(HWND(raw as _)), msg, WPARAM(0), LPARAM(0));
            }
            let _ = PostMessageW(Some(HWND(raw as _)), WM_QUIT_HOST, WPARAM(0), LPARAM(0));
        });
        SetTimer(Some(hwnd), 1, 30, None);

        let p = live.get();
        let mut last = p.state();
        let mut open = false;
        let mut ticks = 0u32;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_TIMER if msg.hwnd == hwnd => {
                    if open {
                        p.idle();
                    }
                    ticks += 1;
                    // Cada segundo, el estado nuevo si ha cambiado (para guardarlo; el sonido ya lo lleva)
                    if ticks % 33 == 0 {
                        send(p, &mut last);
                    }
                }
                WM_SHOW => {
                    if open {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                        let _ = SetForegroundWindow(hwnd);
                    } else if let Some((w, h)) = p.editor_size() {
                        let (ow, oh) = outer(w, h, style);
                        let _ = SetWindowPos(hwnd, None, 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                        let resize = Box::new(move |w: i32, h: i32| {
                            let (ow, oh) = outer(w, h, style);
                            let _ = SetWindowPos(HWND(raw as _), None, 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                        });
                        if p.open_editor(hwnd.0, resize) {
                            open = true;
                            let _ = ShowWindow(hwnd, SW_SHOW);
                            let _ = SetForegroundWindow(hwnd);
                            out("EDITOR 1");
                        } else {
                            out("ERROR noEditor");
                        }
                    } else {
                        out("ERROR noEditor");
                    }
                }
                WM_HIDE => {
                    if open {
                        p.close_editor();
                        open = false;
                    }
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    flush(&live);
                    send(p, &mut last);
                    out("EDITOR 0");
                }
                WM_QUIT_HOST => break,
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        if open {
            p.close_editor();
        }
        stop.store(true, Ordering::SeqCst);
        let _ = audio.join();
        flush(&live);
        send(p, &mut last);
        let _ = DestroyWindow(hwnd);
    }
    0
}

/// Lo tocado en la ventana con el audio parado (si suena, ya le llega en cada bloque)
fn flush(live: &Live) {
    let _g = live.audio.lock().unwrap_or_else(|e| e.into_inner());
    live.get().flush();
}

/// Atiende el puente: espera cada bloque del motor, lo procesa con el plugin y lo devuelve
fn audio_thread(live: &Live, stop: &AtomicBool, key: u32, mut server: Option<Server>, mut rate: f64) {
    pro_audio();
    let (mut l, mut r) = (vec![0.0f32; MAX_BLOCK], vec![0.0f32; MAX_BLOCK]);
    let mut tried = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let Some(s) = &server else {
            // El motor aún no ha cargado esta línea (o se ha reiniciado): se vuelve a mirar cada medio segundo
            std::thread::sleep(Duration::from_millis(50));
            if tried.elapsed() > Duration::from_millis(500) {
                server = Server::open(key);
                tried = Instant::now();
            }
            continue;
        };
        s.serve(100, &mut l, &mut r, |now, l, r| {
            let _g = live.audio.lock().unwrap_or_else(|e| e.into_inner());
            if now >= 8000.0 && (now - rate).abs() > 0.5 {
                live.get().set_rate(now);
                rate = now;
            }
            live.get().process(l, r);
        });
    }
    if let Some(s) = server {
        s.leave();
    }
}

/// Prioridad de audio de Windows (MMCSS «Pro Audio») para el hilo que procesa
fn pro_audio() {
    let mut task = 0u32;
    unsafe {
        let _ = windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task);
    }
}

fn send(plugin: &mut Plugin, last: &mut Vec<u8>) {
    let s = plugin.state();
    if s != *last && s.len() <= poxidsp::host::MAX_STATE {
        out(&format!("STATE {}", poxidsp::base64(&s)));
        *last = s;
    }
}

/// Tamaño de ventana para un área de cliente de `w`×`h`
fn outer(w: i32, h: i32, style: WINDOW_STYLE) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
    unsafe {
        let _ = AdjustWindowRectEx(&mut r, style, false, WINDOW_EX_STYLE::default());
    }
    (r.right - r.left, r.bottom - r.top)
}

/// Cerrar la ventana solo la esconde: el plugin sigue sonando
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLOSE {
        let _ = PostMessageW(Some(hwnd), WM_HIDE, WPARAM(0), LPARAM(0));
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
