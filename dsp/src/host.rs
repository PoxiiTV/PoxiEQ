//! Plugins del usuario (VST2 y VST3) alojados por PoxiDSP.
//!
//! El motor solo sabe cargar VST2, y además «compensa» su latencia corrompiendo la salida (ver `initial_delay`
//! en `vst.rs`). Así que todos los plugins del usuario van dentro de una instancia de PoxiDSP en «modo host»:
//! su ChunkData lleva el formato, la ruta y el estado del plugin (`Spec`), y PoxiDSP lo carga y le pasa el
//! audio (anunciando latencia 0 al motor). El mismo código lo usa la app en un proceso aparte para escanear y
//! para abrir la ventana de edición de cada plugin (ahí sí con su controlador y su editor).

use std::ffi::c_void;
use std::path::{Path, PathBuf};

pub use crate::host3::Vst3;

/// Primeros bytes del ChunkData de una instancia en modo host
pub const MAGIC: &[u8; 4] = b"PXHS";
/// Tope del estado de un plugin (algunos guardan muestras o curvas enormes)
pub const MAX_STATE: usize = 8 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Vst2,
    Vst3,
}

/// Qué plugin cargar y con qué estado
#[derive(Clone, PartialEq, Debug)]
pub struct Spec {
    pub format: Format,
    pub path: String,
    /// Clase de VST3 dentro del módulo (en VST2, ceros)
    pub cid: [u8; 16],
    /// Lo que devolvió `Plugin::state` (vacío: el estado inicial del plugin)
    pub state: Vec<u8>,
}

impl Spec {
    pub fn chunk(&self) -> Vec<u8> {
        let mut c = MAGIC.to_vec();
        c.push(1);
        c.push(if self.format == Format::Vst2 { 2 } else { 3 });
        c.extend_from_slice(&self.cid);
        c.extend_from_slice(&(self.path.len() as u16).to_le_bytes());
        c.extend_from_slice(self.path.as_bytes());
        c.extend_from_slice(&(self.state.len() as u32).to_le_bytes());
        c.extend_from_slice(&self.state);
        c
    }

    pub fn from_chunk(d: &[u8]) -> Option<Spec> {
        let rest = d.strip_prefix(MAGIC.as_slice())?;
        let (&[ver, fmt], rest) = rest.split_first_chunk::<2>()?;
        if ver != 1 {
            return None;
        }
        let format = match fmt {
            2 => Format::Vst2,
            3 => Format::Vst3,
            _ => return None,
        };
        let (cid, rest) = rest.split_first_chunk::<16>()?;
        let (len, rest) = rest.split_first_chunk::<2>()?;
        let len = u16::from_le_bytes(*len) as usize;
        let path = std::str::from_utf8(rest.get(..len)?).ok()?.to_string();
        let rest = &rest[len..];
        let (slen, rest) = rest.split_first_chunk::<4>()?;
        let slen = u32::from_le_bytes(*slen) as usize;
        if slen > MAX_STATE || rest.len() != slen {
            return None;
        }
        Some(Spec { format, path, cid: *cid, state: rest.to_vec() })
    }
}

/// Lo que se sabe de un plugin al escanearlo
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub format: Format,
    pub path: String,
    pub cid: [u8; 16],
    pub name: String,
    pub vendor: String,
    /// Muestras de retardo que añade (a la frecuencia de escaneo, 48 kHz)
    pub latency: u32,
}

/// Un plugin cargado
pub enum Plugin {
    V2(Box<Vst2>),
    V3(Box<Vst3>),
}

impl Plugin {
    /// Para el audio: sin controlador ni editor
    pub fn load(spec: &Spec, rate: f64, block: usize) -> Result<Plugin, String> {
        Self::open(spec, rate, block, false)
    }

    /// Para editarlo (proceso aparte de la app): con su controlador, listo para abrir su ventana
    pub fn load_for_editor(spec: &Spec) -> Result<Plugin, String> {
        Self::open(spec, 48000.0, 512, true)
    }

    fn open(spec: &Spec, rate: f64, block: usize, editor: bool) -> Result<Plugin, String> {
        let block = block.clamp(16, 8192);
        Ok(match spec.format {
            Format::Vst2 => Plugin::V2(Vst2::load(&spec.path, &spec.state, rate, block)?),
            Format::Vst3 => Plugin::V3(Vst3::load(&spec.path, &spec.cid, &spec.state, rate, block, editor)?),
        })
    }

    /// Un bloque estéreo en el sitio (`l.len() == r.len()` ≤ bloque de `load`)
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self {
            Plugin::V2(p) => p.process(l, r),
            Plugin::V3(p) => p.process(l, r),
        }
    }

    pub fn state(&mut self) -> Vec<u8> {
        match self {
            Plugin::V2(p) => p.state(),
            Plugin::V3(p) => p.state(),
        }
    }

    pub fn latency(&self) -> u32 {
        match self {
            Plugin::V2(p) => p.latency(),
            Plugin::V3(p) => p.latency(),
        }
    }

    /// Tamaño de su ventana de edición (si tiene)
    pub fn editor_size(&mut self) -> Option<(i32, i32)> {
        match self {
            Plugin::V2(p) => p.editor_size(),
            Plugin::V3(p) => p.editor_size(),
        }
    }

    /// Mete su editor en la ventana `hwnd`; `resize` se llama cuando el plugin pide otro tamaño
    pub fn open_editor(&mut self, hwnd: *mut c_void, resize: Box<dyn Fn(i32, i32)>) -> bool {
        match self {
            Plugin::V2(p) => p.open_editor(hwnd, resize),
            Plugin::V3(p) => p.open_editor(hwnd, resize),
        }
    }

    /// Puede haber cambiado su estado desde el último `state` (en VST3, solo si se ha tocado algo en el editor;
    /// en VST2 no hay forma fiable de saberlo y hay que comparar)
    pub fn maybe_changed(&self) -> bool {
        match self {
            Plugin::V2(_) => true,
            Plugin::V3(p) => p.edited(),
        }
    }

    /// Cada ~30 ms con la ventana abierta (VST2 lo necesita para repintar)
    pub fn idle(&mut self) {
        if let Plugin::V2(p) = self {
            p.idle()
        }
    }

    pub fn close_editor(&mut self) {
        match self {
            Plugin::V2(p) => p.close_editor(),
            Plugin::V3(p) => p.close_editor(),
        }
    }
}

/// Plugins de un archivo (un VST3 puede traer varios efectos). Carga código ajeno: la app lo hace en un proceso
/// aparte con tiempo límite.
pub fn scan(path: &str) -> Result<Vec<Info>, String> {
    if path.to_ascii_lowercase().ends_with(".vst3") {
        crate::host3::scan(path)
    } else {
        Vst2::scan(path).map(|i| vec![i])
    }
}

// ------------------------------------------------------------------ módulos

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

/// Busca las DLL que necesite el plugin en su propia carpeta
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;

/// DLL cargada; se suelta al final (después de soltar todo lo que creó)
pub(crate) struct Library(*mut c_void);

impl Library {
    pub(crate) fn load(path: &Path) -> Result<Library, String> {
        let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
        let h = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        if h.is_null() {
            Err(format!("no se puede cargar {}", path.display()))
        } else {
            Ok(Library(h))
        }
    }

    /// `name` acaba en `\0`
    pub(crate) fn symbol(&self, name: &[u8]) -> *mut c_void {
        unsafe { GetProcAddress(self.0, name.as_ptr()) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

/// Un VST3 puede ser un archivo suelto o una carpeta (`X.vst3\Contents\x86_64-win\X.vst3`)
pub(crate) fn vst3_binary(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_dir() {
        if let Some(name) = p.file_name() {
            return p.join("Contents").join("x86_64-win").join(name);
        }
    }
    p
}

pub(crate) fn c_text(bytes: &[i8]) -> String {
    let b: Vec<u8> = bytes.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&b).trim().to_string()
}

// --------------------------------------------------------------------- VST2

type HostCb = extern "C" fn(*mut Fx, i32, i32, isize, *mut c_void, f32) -> isize;
type Dispatcher = extern "C" fn(*mut Fx, i32, i32, isize, *mut c_void, f32) -> isize;
type ProcessF32 = extern "C" fn(*mut Fx, *const *const f32, *mut *mut f32, i32);
type ProcessF64 = extern "C" fn(*mut Fx, *const *const f64, *mut *mut f64, i32);
type SetParameter = extern "C" fn(*mut Fx, i32, f32);
type GetParameter = extern "C" fn(*mut Fx, i32) -> f32;

/// `AEffect` de VST 2.4 visto desde el host
#[repr(C)]
pub struct Fx {
    magic: i32,
    dispatcher: Dispatcher,
    process: usize,
    set_parameter: SetParameter,
    get_parameter: GetParameter,
    num_programs: i32,
    num_params: i32,
    num_inputs: i32,
    num_outputs: i32,
    flags: i32,
    reserved: [isize; 2],
    initial_delay: i32,
    qualities: [i32; 2],
    io_ratio: f32,
    object: usize,
    user: usize,
    unique_id: i32,
    version: i32,
    process_replacing: Option<ProcessF32>,
    process_double_replacing: Option<ProcessF64>,
}

const VST_MAGIC: i32 = i32::from_be_bytes(*b"VstP");
const FLAG_EDITOR: i32 = 1;
const FLAG_REPLACING: i32 = 1 << 4;
const FLAG_CHUNKS: i32 = 1 << 5;
const FLAG_SYNTH: i32 = 1 << 8;
const OPEN: i32 = 0;
const CLOSE: i32 = 1;
const SET_SAMPLE_RATE: i32 = 10;
const SET_BLOCK_SIZE: i32 = 11;
const MAINS_CHANGED: i32 = 12;
const EDIT_GET_RECT: i32 = 13;
const EDIT_OPEN: i32 = 14;
const EDIT_CLOSE: i32 = 15;
const EDIT_IDLE: i32 = 19;
const GET_CHUNK: i32 = 23;
const SET_CHUNK: i32 = 24;
const GET_PLUG_CATEGORY: i32 = 35;
const GET_EFFECT_NAME: i32 = 45;
const GET_VENDOR_STRING: i32 = 47;
const START_PROCESS: i32 = 71;
const STOP_PROCESS: i32 = 72;
const CATEGORY_SHELL: isize = 10;

type Resize = Option<Box<dyn Fn(i32, i32)>>;

thread_local! {
    /// Tamaño de ventana que pide el plugin (`audioMasterSizeWindow`): un editor por proceso, en el hilo de su
    /// ventana (que es desde donde lo pide)
    static RESIZE: std::cell::RefCell<Resize> = const { std::cell::RefCell::new(None) };
}

extern "C" fn host_callback(_fx: *mut Fx, op: i32, index: i32, value: isize, ptr: *mut c_void, _opt: f32) -> isize {
    match op {
        1 => 2400, // audioMasterVersion
        6 => 1,    // audioMasterWantMidi (se ignora)
        15 => {
            // audioMasterSizeWindow
            RESIZE.with(|r| match r.try_borrow().ok().as_ref().and_then(|r| r.as_ref()) {
                Some(f) => {
                    f(index, value as i32);
                    1
                }
                None => 0,
            })
        }
        32 => copy_text(ptr, "Poxi"),   // audioMasterGetVendorString
        33 => copy_text(ptr, "PoxiEQ"), // audioMasterGetProductString
        34 => 1,                        // audioMasterGetVendorVersion
        38 => 2,                        // audioMasterGetLanguage (inglés)
        42..=44 => 1,                   // updateDisplay, beginEdit, endEdit
        _ => 0,
    }
}

fn copy_text(ptr: *mut c_void, s: &str) -> isize {
    if ptr.is_null() {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), ptr as *mut u8, s.len());
        *(ptr as *mut u8).add(s.len()) = 0;
    }
    1
}

pub struct Vst2 {
    fx: *mut Fx,
    ins: Vec<Vec<f32>>,
    outs: Vec<Vec<f32>>,
    editor: bool,
    _lib: Library,
}

// El host de audio lo usa desde los hilos del motor, de uno en uno
unsafe impl Send for Vst2 {}

impl Vst2 {
    fn instantiate(path: &str) -> Result<(Library, *mut Fx), String> {
        let lib = Library::load(Path::new(path))?;
        let mut main = lib.symbol(b"VSTPluginMain\0");
        if main.is_null() {
            main = lib.symbol(b"main\0");
        }
        if main.is_null() {
            return Err("no es un plugin VST2".into());
        }
        let main: extern "C" fn(HostCb) -> *mut Fx = unsafe { std::mem::transmute(main) };
        let fx = main(host_callback);
        if fx.is_null() || unsafe { (*fx).magic } != VST_MAGIC {
            return Err("no es un plugin VST2".into());
        }
        Ok((lib, fx))
    }

    fn call(&self, op: i32, index: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
        unsafe { ((*self.fx).dispatcher)(self.fx, op, index, value, ptr, opt) }
    }

    fn scan(path: &str) -> Result<Info, String> {
        let (lib, fx) = Self::instantiate(path)?;
        let p = Vst2 { fx, ins: vec![], outs: vec![], editor: false, _lib: lib };
        p.call(OPEN, 0, 0, std::ptr::null_mut(), 0.0);
        let f = unsafe { &*fx };
        if p.call(GET_PLUG_CATEGORY, 0, 0, std::ptr::null_mut(), 0.0) == CATEGORY_SHELL {
            return Err("los «shell» (varios plugins en uno) no se pueden usar".into());
        }
        if f.flags & FLAG_SYNTH != 0 {
            return Err("es un instrumento, no un efecto".into());
        }
        if f.num_outputs < 2 {
            return Err("solo plugins estéreo".into());
        }
        let text = |op| {
            let mut buf = [0i8; 256];
            p.call(op, 0, 0, buf.as_mut_ptr() as *mut c_void, 0.0);
            c_text(&buf)
        };
        let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = Some(text(GET_EFFECT_NAME)).filter(|n| !n.is_empty()).unwrap_or(stem);
        let vendor = text(GET_VENDOR_STRING);
        drop(p);
        // La latencia se sabe con el plugin ya preparado
        let latency = Vst2::load(path, &[], 48000.0, 512).map(|p| p.latency()).unwrap_or(0);
        Ok(Info { format: Format::Vst2, path: path.into(), cid: [0; 16], name, vendor, latency })
    }

    fn load(path: &str, state: &[u8], rate: f64, block: usize) -> Result<Box<Vst2>, String> {
        let (lib, fx) = Self::instantiate(path)?;
        let f = unsafe { &*fx };
        if f.num_outputs < 2 || (f.process_replacing.is_none() || f.flags & FLAG_REPLACING == 0) {
            return Err("plugin no compatible".into());
        }
        let p = Box::new(Vst2 {
            fx,
            ins: vec![vec![0.0; block]; f.num_inputs.max(2) as usize],
            outs: vec![vec![0.0; block]; f.num_outputs as usize],
            editor: false,
            _lib: lib,
        });
        p.call(OPEN, 0, 0, std::ptr::null_mut(), 0.0);
        p.call(SET_SAMPLE_RATE, 0, 0, std::ptr::null_mut(), rate as f32);
        p.call(SET_BLOCK_SIZE, 0, block as isize, std::ptr::null_mut(), 0.0);
        p.restore(state);
        p.call(MAINS_CHANGED, 0, 1, std::ptr::null_mut(), 0.0);
        p.call(START_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
        Ok(p)
    }

    /// Estado: `0` + chunk del banco, o `1` + parámetros (f32) si el plugin no tiene chunks
    fn restore(&self, state: &[u8]) {
        match state.split_first() {
            Some((0, chunk)) if !chunk.is_empty() => {
                self.call(SET_CHUNK, 0, chunk.len() as isize, chunk.as_ptr() as *mut c_void, 0.0);
            }
            Some((1, params)) => {
                let f = unsafe { &*self.fx };
                for (i, v) in params.chunks_exact(4).take(f.num_params.max(0) as usize).enumerate() {
                    (f.set_parameter)(self.fx, i as i32, f32::from_le_bytes([v[0], v[1], v[2], v[3]]));
                }
            }
            _ => {}
        }
    }

    fn state(&mut self) -> Vec<u8> {
        let f = unsafe { &*self.fx };
        if f.flags & FLAG_CHUNKS != 0 {
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let len = self.call(GET_CHUNK, 0, 0, &mut ptr as *mut _ as *mut c_void, 0.0);
            if !ptr.is_null() && len > 0 && (len as usize) < MAX_STATE {
                let mut s = vec![0u8];
                s.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len as usize) });
                return s;
            }
        }
        let mut s = vec![1u8];
        for i in 0..f.num_params.max(0) {
            s.extend_from_slice(&(f.get_parameter)(self.fx, i).to_le_bytes());
        }
        s
    }

    fn latency(&self) -> u32 {
        unsafe { (*self.fx).initial_delay.max(0) as u32 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len()).min(self.outs[0].len());
        self.ins[0][..n].copy_from_slice(&l[..n]);
        self.ins[1][..n].copy_from_slice(&r[..n]);
        let inp: Vec<*const f32> = self.ins.iter().map(|b| b.as_ptr()).collect();
        let mut out: Vec<*mut f32> = self.outs.iter_mut().map(|b| b.as_mut_ptr()).collect();
        // ponytail: dos Vec de punteros por bloque; guardarlos en la instancia si alguna vez pesa
        if let Some(f) = unsafe { (*self.fx).process_replacing } {
            f(self.fx, inp.as_ptr(), out.as_mut_ptr(), n as i32);
            l[..n].copy_from_slice(&self.outs[0][..n]);
            r[..n].copy_from_slice(&self.outs[1][..n]);
        }
    }

    fn editor_size(&mut self) -> Option<(i32, i32)> {
        if unsafe { (*self.fx).flags } & FLAG_EDITOR == 0 {
            return None;
        }
        #[repr(C)]
        struct ERect {
            top: i16,
            left: i16,
            bottom: i16,
            right: i16,
        }
        let mut rect: *mut ERect = std::ptr::null_mut();
        self.call(EDIT_GET_RECT, 0, 0, &mut rect as *mut _ as *mut c_void, 0.0);
        let r = unsafe { rect.as_ref() }?;
        Some(((r.right - r.left) as i32, (r.bottom - r.top) as i32)).filter(|&(w, h)| w > 0 && h > 0)
    }

    fn open_editor(&mut self, hwnd: *mut c_void, resize: Box<dyn Fn(i32, i32)>) -> bool {
        RESIZE.with(|r| *r.borrow_mut() = Some(resize));
        self.editor = self.call(EDIT_OPEN, 0, 0, hwnd, 0.0) != 0 || self.editor_size().is_some();
        self.editor
    }

    fn idle(&mut self) {
        if self.editor {
            self.call(EDIT_IDLE, 0, 0, std::ptr::null_mut(), 0.0);
        }
    }

    fn close_editor(&mut self) {
        if std::mem::take(&mut self.editor) {
            self.call(EDIT_CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
        }
    }
}

impl Drop for Vst2 {
    fn drop(&mut self) {
        self.close_editor();
        self.call(STOP_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
        self.call(MAINS_CHANGED, 0, 0, std::ptr::null_mut(), 0.0);
        self.call(CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_chunk_de_host_va_y_vuelve() {
        let s = Spec { format: Format::Vst3, path: r"C:\Program Files\Common Files\VST3\Ñu.vst3".into(), cid: [7; 16], state: vec![1, 2, 3] };
        let c = s.chunk();
        assert_eq!(Spec::from_chunk(&c), Some(s));
        assert_eq!(Spec::from_chunk(&c[..c.len() - 1]), None);
        assert_eq!(Spec::from_chunk(b"PXHS\x02"), None);
    }

    /// Con plugins de verdad (los del PC del dueño; en memoria, no suena nada):
    /// `cargo test -- --ignored plugins_reales --nocapture`
    #[test]
    #[ignore]
    fn plugins_reales() {
        for path in [r"C:\Program Files\Steinberg\VstPlugins\TDR Nova.dll", r"C:\Program Files\Common Files\VST3\ValhallaDelay.vst3"] {
            if !Path::new(path).exists() {
                continue;
            }
            let infos = scan(path).expect("escanear");
            println!("{path}: {infos:?}");
            let i = &infos[0];
            let spec = Spec { format: i.format, path: path.into(), cid: i.cid, state: vec![] };
            let mut p = Plugin::load(&spec, 48000.0, 480).expect("cargar");
            let mut l: Vec<f32> = (0..480).map(|n| (n as f32 * 0.05).sin() * 0.5).collect();
            let mut r = l.clone();
            for _ in 0..20 {
                p.process(&mut l, &mut r);
            }
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
            let state = p.state();
            assert!(!state.is_empty());
            drop(p);
            let mut p = Plugin::load(&Spec { state: state.clone(), ..spec.clone() }, 44100.0, 512).expect("recargar con estado");
            p.process(&mut l, &mut r);
            println!("  estado {} bytes, latencia {}", state.len(), p.latency());
            drop(p);
            let mut e = Plugin::load_for_editor(&Spec { state, ..spec }).expect("cargar para editar");
            println!("  editor {:?}", e.editor_size());
            assert!(!e.state().is_empty());
        }
    }
}
