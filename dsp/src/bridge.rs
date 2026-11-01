//! Puente de audio para los plugins del usuario: PoxiDSP (en el motor, dentro de audiodg, sin escritorio) le pasa
//! cada bloque al proceso de PoxiEQ que aloja de verdad el plugin (en la sesión del usuario, con su ventana) y
//! espera el resultado. Así la ventana que se ve es la del plugin que suena.
//!
//! Por hueco de plugin: una memoria compartida con el bloque (se procesa en el sitio) y dos eventos (`req`: hay
//! bloque; `done`: ya está). Los crea PoxiDSP (el motor tiene permiso para `Global\`) y el proceso del plugin los
//! abre. Si no responde a tiempo (no está, se ha colgado o el plugin no da abasto), el bloque pasa sin procesar.

use crate::shm::Shared;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Muestras por bloque como mucho (por canal)
pub const MAX_BLOCK: usize = 8192;

#[repr(C)]
pub struct Block {
    /// El proceso del plugin está ahí (lo pone a cada vuelta; el motor lo borra si deja de responder)
    pub ready: AtomicU32,
    pub rate: AtomicU32,
    pub frames: AtomicU32,
    /// Bloques pedidos y bloques hechos
    pub seq: AtomicU32,
    pub done: AtomicU32,
    /// L y R intercaladas, f32 en bits: entra el audio y sale procesado
    pub buf: [AtomicU32; MAX_BLOCK * 2],
}

type Handle = *mut c_void;

#[repr(C)]
struct SecurityAttributes {
    len: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

const WAIT_OBJECT_0: u32 = 0;
const SYNCHRONIZE: u32 = 0x0010_0000;
const EVENT_MODIFY_STATE: u32 = 0x0002;
/// Sistema, servicio local (el motor), administradores y el usuario de la sesión (el proceso del plugin)
const SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;LS)(A;;GA;;;BA)(A;;GA;;;IU)";

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventW(sa: *const SecurityAttributes, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn SetEvent(h: Handle) -> i32;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn CloseHandle(h: Handle) -> i32;
    fn LocalFree(mem: *mut c_void) -> *mut c_void;
}

#[link(name = "advapi32")]
extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl: *const u16, rev: u32, sd: *mut *mut c_void, size: *mut u32) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// En los tests no hay permiso para `Global\`: mismo código con nombres de la sesión (también si el proceso lleva
/// `POXIEQ_BRIDGE_LOCAL`, para probar el circuito entero sin el motor)
fn prefix() -> &'static str {
    if cfg!(test) || std::env::var_os("POXIEQ_BRIDGE_LOCAL").is_some() {
        "Local\\"
    } else {
        "Global\\"
    }
}

fn name(key: u32, what: &str) -> String {
    format!("{}PoxiDSP-bridge-{key:08x}{what}", prefix())
}

struct Event(Handle);

unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    fn create(name: &str) -> Option<Event> {
        let sddl = wide(SDDL);
        let name = wide(name);
        unsafe {
            let mut sd = std::ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut sd, std::ptr::null_mut()) == 0 {
                return None;
            }
            let sa = SecurityAttributes { len: size_of::<SecurityAttributes>() as u32, descriptor: sd, inherit: 0 };
            let h = CreateEventW(&sa, 0, 0, name.as_ptr());
            LocalFree(sd);
            (!h.is_null()).then_some(Event(h))
        }
    }

    fn open(name: &str) -> Option<Event> {
        let name = wide(name);
        let h = unsafe { OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        (!h.is_null()).then_some(Event(h))
    }

    fn set(&self) {
        unsafe { SetEvent(self.0) };
    }

    /// `true` si llegó la señal antes de `ms`
    fn wait(&self, ms: u32) -> bool {
        unsafe { WaitForSingleObject(self.0, ms) == WAIT_OBJECT_0 }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

// ---------------------------------------------------------------- en el motor

/// Lado de PoxiDSP: manda cada bloque y espera el resultado
pub struct Client {
    map: Shared<Block>,
    req: Event,
    done: Event,
    misses: u32,
}

impl Client {
    pub fn create(key: u32) -> Option<Client> {
        Some(Client { map: Shared::create_with(&name(key, ""), SDDL)?, req: Event::create(&name(key, "-req"))?, done: Event::create(&name(key, "-done"))?, misses: 0 })
    }

    /// Procesa un bloque en el sitio por el proceso del plugin. `false` si no ha llegado a tiempo (el bloque se
    /// queda tal cual: pasa sin procesar).
    pub fn process(&mut self, rate: f64, l: &mut [f32], r: &mut [f32]) -> bool {
        let b = self.map.get();
        b.rate.store(rate as u32, Ordering::Relaxed);
        let n = l.len().min(r.len()).min(MAX_BLOCK);
        if n == 0 || b.ready.load(Ordering::Acquire) == 0 {
            return false;
        }
        for i in 0..n {
            b.buf[i * 2].store(l[i].to_bits(), Ordering::Relaxed);
            b.buf[i * 2 + 1].store(r[i].to_bits(), Ordering::Relaxed);
        }
        b.frames.store(n as u32, Ordering::Relaxed);
        let seq = b.seq.load(Ordering::Relaxed).wrapping_add(1);
        b.seq.store(seq, Ordering::Release);
        self.req.set();
        // Como mucho el 80 % de lo que dura el bloque (entre 2 y 20 ms): más, y el motor perdería su turno
        let budget = Duration::from_secs_f64((n as f64 / rate.max(8000.0) * 0.8).clamp(0.002, 0.02));
        let deadline = Instant::now() + budget;
        loop {
            if b.done.load(Ordering::Acquire) == seq {
                for i in 0..n {
                    l[i] = sane(b.buf[i * 2].load(Ordering::Relaxed));
                    r[i] = sane(b.buf[i * 2 + 1].load(Ordering::Relaxed));
                }
                self.misses = 0;
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            // Un `done` atrasado de un bloque anterior despierta sin ser el nuestro: se vuelve a esperar
            if left.is_zero() || (!self.done.wait(left.as_millis().max(1) as u32) && Instant::now() >= deadline) {
                break;
            }
        }
        // Tres seguidos sin respuesta: se da por ausente (el proceso lo vuelve a marcar en cuanto responda)
        self.misses += 1;
        if self.misses >= 3 {
            b.ready.store(0, Ordering::Release);
            self.misses = 0;
        }
        false
    }
}

fn sane(bits: u32) -> f32 {
    let x = f32::from_bits(bits);
    if x.is_finite() {
        x.clamp(-16.0, 16.0)
    } else {
        0.0
    }
}

// ------------------------------------------------------ en el proceso del plugin

/// Lado del proceso que aloja el plugin: espera bloques, los procesa y avisa
pub struct Server {
    map: Shared<Block>,
    req: Event,
    done: Event,
}

impl Server {
    /// `None` mientras el motor no haya creado el puente (aún no ha cargado esa línea de configuración)
    pub fn open(key: u32) -> Option<Server> {
        Some(Server { map: Shared::open_rw(&name(key, ""))?, req: Event::open(&name(key, "-req"))?, done: Event::open(&name(key, "-done"))? })
    }

    pub fn rate(&self) -> f64 {
        self.map.get().rate.load(Ordering::Relaxed) as f64
    }

    /// Espera un bloque hasta `ms` y lo procesa con `f(frecuencia, l, r)`. `l` y `r` son búferes del llamador
    /// (sin reservas aquí). `true` si había bloque.
    pub fn serve(&self, ms: u32, l: &mut [f32], r: &mut [f32], f: impl FnOnce(f64, &mut [f32], &mut [f32])) -> bool {
        let b = self.map.get();
        b.ready.store(1, Ordering::Release);
        if !self.req.wait(ms) {
            return false;
        }
        let seq = b.seq.load(Ordering::Acquire);
        let n = (b.frames.load(Ordering::Relaxed) as usize).min(MAX_BLOCK).min(l.len()).min(r.len());
        for i in 0..n {
            l[i] = sane(b.buf[i * 2].load(Ordering::Relaxed));
            r[i] = sane(b.buf[i * 2 + 1].load(Ordering::Relaxed));
        }
        f(self.rate(), &mut l[..n], &mut r[..n]);
        for i in 0..n {
            b.buf[i * 2].store(l[i].to_bits(), Ordering::Relaxed);
            b.buf[i * 2 + 1].store(r[i].to_bits(), Ordering::Relaxed);
        }
        b.done.store(seq, Ordering::Release);
        self.done.set();
        true
    }

    /// Al cerrar: el motor deja de esperar al momento
    pub fn leave(&self) {
        self.map.get().ready.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_y_vuelta_y_sin_proceso_pasa_tal_cual() {
        let key = std::process::id() ^ 0x5a5a_0000;
        let mut c = Client::create(key).expect("crear");
        let (mut l, mut r) = (vec![0.5f32; 480], vec![-0.25f32; 480]);
        // Sin nadie al otro lado: no espera nada y lo deja igual
        let t = Instant::now();
        assert!(!c.process(48000.0, &mut l, &mut r));
        assert!(t.elapsed() < Duration::from_millis(5));
        assert_eq!((l[0], r[0]), (0.5, -0.25));

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = stop.clone();
        let host = std::thread::spawn(move || {
            let s = Server::open(key).expect("abrir");
            let (mut bl, mut br) = (vec![0.0f32; MAX_BLOCK], vec![0.0f32; MAX_BLOCK]);
            while !s2.load(Ordering::SeqCst) {
                s.serve(50, &mut bl, &mut br, |rate, l, r| {
                    assert_eq!(rate, 48000.0);
                    l.iter_mut().for_each(|v| *v = -*v);
                    r.iter_mut().for_each(|v| *v *= 2.0);
                });
            }
            s.leave();
        });
        // Hasta que el proceso del plugin marque que está
        while c.map.get().ready.load(Ordering::Acquire) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..50 {
            let (mut l, mut r) = (vec![0.5f32; 480], vec![-0.25f32; 480]);
            assert!(c.process(48000.0, &mut l, &mut r));
            assert_eq!((l[100], r[100]), (-0.5, -0.5));
        }
        stop.store(true, Ordering::SeqCst);
        host.join().unwrap();
        let (mut l, mut r) = (vec![0.5f32; 480], vec![0.0f32; 480]);
        assert!(!c.process(48000.0, &mut l, &mut r));
        assert_eq!(l[0], 0.5);
    }
}

#[cfg(test)]
mod real {
    use super::*;
    use std::io::{BufRead, Write};

    /// El circuito entero con el exe de PoxiEQ y un VST3 de verdad (en memoria, no suena nada). Hace falta
    /// `cargo build` en src-tauri antes: `cargo test -- --ignored circuito_entero --nocapture`
    #[test]
    #[ignore]
    fn circuito_entero() {
        let exe = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\src-tauri\target\debug\poxieq.exe");
        let plugin = r"C:\Program Files\Common Files\VST3\ValhallaDelay.vst3";
        let cid: String = b"dTSVaLvyalhallad".iter().map(|b| format!("{b:02x}")).collect();
        let key = 0x5100_0000 ^ std::process::id();
        let job = format!(
            r#"{{"slot":{{"id":"t1","format":"vst3","path":{:?},"cid":"{cid}","name":"ValhallaDelay","vendor":"","on":true,"state":"","latency":0}},"title":"t","bridge":{key}}}"#,
            plugin
        );
        let file = std::env::temp_dir().join(format!("poxieq-bridge-test-{key}.json"));
        std::fs::write(&file, job).unwrap();
        // El motor crea el puente antes de que llegue el proceso
        let mut c = Client::create(key).expect("crear");
        let mut child = std::process::Command::new(exe)
            .args(["--plugin-host", "serve"])
            .arg(&file)
            .env("POXIEQ_BRIDGE_LOCAL", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("arrancar el exe (¿cargo build en src-tauri?)");
        let t = Instant::now();
        while c.map.get().ready.load(Ordering::Acquire) == 0 {
            assert!(t.elapsed() < Duration::from_secs(15), "el proceso no se engancha al puente");
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("enganchado en {:?}", t.elapsed());
        let (mut ok, mut changed, mut worst) = (0, 0, Duration::ZERO);
        for b in 0..300 {
            let mut l: Vec<f32> = (0..480).map(|i| ((b * 480 + i) as f32 * 0.06).sin() * 0.5).collect();
            let mut r = l.clone();
            let before = l.clone();
            let t = Instant::now();
            if c.process(48000.0, &mut l, &mut r) {
                ok += 1;
                worst = worst.max(t.elapsed());
                if l.iter().zip(&before).any(|(a, b)| (a - b).abs() > 1e-4) {
                    changed += 1;
                }
            }
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
            std::thread::sleep(Duration::from_millis(2));
        }
        println!("bloques procesados {ok}/300, cambiados por el plugin {changed}, peor ida y vuelta {worst:?}");
        assert!(ok >= 290 && changed > 100);
        // Su retardo lo manda cada segundo
        std::thread::sleep(Duration::from_millis(1500));
        writeln!(child.stdin.as_mut().unwrap(), "QUIT").unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok).collect::<Vec<_>>();
        assert!(child.wait().unwrap().success());
        println!("salida: {:?}", out.iter().map(|l| &l[..l.len().min(40)]).collect::<Vec<_>>());
        assert!(out.iter().any(|l| l.starts_with("LATENCY ")), "manda su retardo");
        // Sin el proceso, vuelve a pasar tal cual sin esperar
        let (mut l, mut r) = (vec![0.3f32; 480], vec![0.3f32; 480]);
        assert!(!c.process(48000.0, &mut l, &mut r));
        assert_eq!(l[0], 0.3);
    }
}
