//! Proceso aparte para los plugins del usuario (`poxieq.exe --plugin-host …`): escanearlos y abrir su ventana
//! de edición. Las dos cosas cargan código ajeno: si un plugin se cuelga o revienta, cae este proceso y no
//! PoxiEQ (que además le pone tiempo límite al escaneo).
//!
//! - `scan <ruta>`: escribe en la salida estándar un JSON con lo que trae el archivo (o `{"error": …}`).
//! - `edit <archivo.json>`: abre el editor del plugin descrito en el archivo (que se borra al leerlo) y, mientras
//!   está abierto, escribe `STATE <base64>` cada vez que cambia su estado (como mucho dos veces por segundo).

use serde::{Deserialize, Serialize};
use std::io::Write;
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

/// Lo que necesita el editor
#[derive(Serialize, Deserialize)]
pub struct EditJob {
    pub slot: crate::eq::PluginSlot,
    pub title: String,
}

pub fn main(args: &[String]) -> i32 {
    match args {
        [cmd, path] if cmd == "scan" => scan(path),
        [cmd, file] if cmd == "edit" => edit(file),
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

fn edit(file: &str) -> i32 {
    let job: Option<EditJob> = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok());
    let _ = std::fs::remove_file(file);
    let Some(job) = job else { return 2 };
    let Some(spec) = job.slot.spec() else { return 2 };
    let mut plugin = match poxidsp::host::Plugin::load_for_editor(&spec) {
        Ok(p) => p,
        Err(e) => {
            out(&format!("ERROR {e}"));
            return 1;
        }
    };
    let Some((w, h)) = plugin.editor_size() else {
        out("ERROR noEditor");
        return 1;
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
        let (ow, oh) = outer(w, h, style);
        let title = HSTRING::from(job.title.as_str());
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE::default(), class, &title, style, CW_USEDEFAULT, CW_USEDEFAULT, ow, oh, None, None, Some(inst), None) else {
            return 1;
        };
        let raw = hwnd.0 as isize;
        let resize = Box::new(move |w: i32, h: i32| {
            let (ow, oh) = outer(w, h, style);
            let _ = SetWindowPos(HWND(raw as _), None, 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        });
        if !plugin.open_editor(hwnd.0, resize) {
            out("ERROR noEditor");
            let _ = DestroyWindow(hwnd);
            return 1;
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        SetTimer(Some(hwnd), 1, 30, None);

        let mut last = plugin.state();
        let mut ticks = 0u32;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.hwnd == hwnd {
                plugin.idle();
                ticks += 1;
                // Cada ~0,5 s, el estado nuevo si ha cambiado (el motor recarga el plugin con él)
                if ticks % 16 == 0 && plugin.maybe_changed() {
                    send(&mut plugin, &mut last);
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        send(&mut plugin, &mut last);
        plugin.close_editor();
    }
    0
}

fn send(plugin: &mut poxidsp::host::Plugin, last: &mut Vec<u8>) {
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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
