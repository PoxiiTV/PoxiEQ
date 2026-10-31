//! ABI de VST 2.4 a mano (la estructura `AEffect` y los opcodes que usa el motor), sin dependencias.
//!
//! Secuencia del motor por instancia (`VSTPluginInstance.cpp`): `VSTPluginMain` → open → sample rate →
//! block size → set chunk → resume (mains on) → start process → proceso (doble precisión si lo
//! anunciamos) … → stop process → suspend → close. Todo lo que no es proceso llega desde su hilo de
//! carga de configuración, así que ahí sí se reserva memoria; en el proceso, nunca.
//!
//! Ninguna llamada deja escapar un panic: todas pasan por `catch_unwind` y, si algo falla, el audio
//! sale tal cual entró (si el plugin calla, Windows se queda sin sonido).

use crate::dsp::Chain;
use crate::shm::{self, Map};
use crate::Settings;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const MAGIC: i32 = i32::from_be_bytes(*b"VstP");
const UNIQUE_ID: i32 = i32::from_be_bytes(*b"PxDs");

const FLAG_REPLACING: i32 = 1 << 4;
const FLAG_CHUNKS: i32 = 1 << 5;
const FLAG_DOUBLE: i32 = 1 << 12;

const OPEN: i32 = 0;
const CLOSE: i32 = 1;
const SET_SAMPLE_RATE: i32 = 10;
const MAINS_CHANGED: i32 = 12;
const GET_CHUNK: i32 = 23;
const SET_CHUNK: i32 = 24;
const GET_PLUG_CATEGORY: i32 = 35;
const GET_EFFECT_NAME: i32 = 45;
const GET_VENDOR_STRING: i32 = 47;
const GET_PRODUCT_STRING: i32 = 48;
const GET_VENDOR_VERSION: i32 = 49;
const GET_VST_VERSION: i32 = 58;

type HostCallback = Option<extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize>;
type Dispatcher = extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
type Process<T> = extern "C" fn(*mut AEffect, *const *const T, *mut *mut T, i32);
type SetParameter = extern "C" fn(*mut AEffect, i32, f32);
type GetParameter = extern "C" fn(*mut AEffect, i32) -> f32;

/// `AEffect` de VST 2.4, campo a campo (x64)
#[repr(C)]
pub struct AEffect {
    magic: i32,
    dispatcher: Dispatcher,
    process: Process<f32>,
    set_parameter: SetParameter,
    get_parameter: GetParameter,
    num_programs: i32,
    num_params: i32,
    num_inputs: i32,
    num_outputs: i32,
    flags: i32,
    reserved1: isize,
    reserved2: isize,
    /// Latencia anunciada. Siempre 0: el motor (1.4.2) «compensa» la latencia que se le anuncia
    /// copiando con solapes en el mismo búfer y estropea la salida. El lookahead del limitador
    /// (≤ 2 ms) es un retardo fijo de toda la salida, sin nada con lo que desalinearse.
    initial_delay: i32,
    real_qualities: i32,
    off_qualities: i32,
    io_ratio: f32,
    object: *mut c_void,
    user: *mut c_void,
    unique_id: i32,
    version: i32,
    process_replacing: Process<f32>,
    process_double_replacing: Process<f64>,
    future: [u8; 56],
}

/// Estado del nivelador por dispositivo, para la instancia que sustituye a la actual (el motor crea
/// una nueva en cada cambio de configuración y las funde durante 10 ms). Todas las instancias viven
/// en el mismo proceso y la DLL no se descarga mientras haya una.
struct Saved {
    key: AtomicU32,
    /// f64 en bits: (energía, ganancia en dB) del nivelador y del de después del compresor
    state: [AtomicU64; 4],
}

static SAVED: [Saved; shm::SLOTS] = [const { Saved { key: AtomicU32::new(0), state: [const { AtomicU64::new(0) }; 4] } }; shm::SLOTS];

fn saved(key: u32) -> Option<&'static Saved> {
    shm::find_by(&SAVED, |s| &s.key, key, true)
}

#[repr(C)]
struct Plugin {
    /// Lo único que ve el host; va primero para poder volver de `*mut AEffect` a `*mut Plugin`
    effect: AEffect,
    settings: Settings,
    key: u32,
    chunk: Vec<u8>,
    rate: f64,
    chain: Option<Chain>,
    meters: Option<Map>,
    /// Un panic dentro del proceso: desde ahí, paso directo
    failed: bool,
}

impl Plugin {
    fn new() -> Box<Self> {
        Box::new(Plugin {
            effect: AEffect {
                magic: MAGIC,
                dispatcher,
                // El proceso acumulativo (obsoleto) no lo usa el motor con FLAG_REPLACING
                process: process_f32,
                set_parameter,
                get_parameter,
                num_programs: 1,
                num_params: 0,
                num_inputs: 2,
                num_outputs: 2,
                flags: FLAG_REPLACING | FLAG_CHUNKS | FLAG_DOUBLE,
                reserved1: 0,
                reserved2: 0,
                initial_delay: 0,
                real_qualities: 0,
                off_qualities: 0,
                io_ratio: 1.0,
                object: std::ptr::null_mut(),
                user: std::ptr::null_mut(),
                unique_id: UNIQUE_ID,
                version: 1,
                process_replacing: process_f32,
                process_double_replacing: process_f64,
                future: [0; 56],
            },
            settings: Settings::default(),
            key: 0,
            chunk: Vec::new(),
            rate: 44100.0,
            chain: None,
            meters: None,
            failed: false,
        })
    }

    fn dispatch(&mut self, op: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
        match op {
            OPEN => 0,
            SET_SAMPLE_RATE => {
                self.rate = opt as f64;
                0
            }
            SET_CHUNK => {
                if ptr.is_null() || !(1..=1 << 16).contains(&value) {
                    return 0;
                }
                let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, value as usize) };
                // Un chunk que no se entiende deja todo apagado: paso directo
                (self.settings, self.key) = Settings::from_chunk(data).unwrap_or_default();
                self.chunk = data.to_vec();
                1
            }
            GET_CHUNK => {
                if !ptr.is_null() {
                    unsafe { *(ptr as *mut *mut u8) = self.chunk.as_mut_ptr() };
                }
                self.chunk.len() as isize
            }
            MAINS_CHANGED if value != 0 => {
                self.resume();
                0
            }
            GET_PLUG_CATEGORY => 1,
            GET_EFFECT_NAME | GET_PRODUCT_STRING => text(ptr, "PoxiDSP"),
            GET_VENDOR_STRING => text(ptr, "Poxi"),
            GET_VENDOR_VERSION => 1,
            GET_VST_VERSION => 2400,
            _ => 0,
        }
    }

    /// Prepara la cadena con los ajustes y la frecuencia ya recibidos (aquí sí se reserva memoria)
    fn resume(&mut self) {
        self.chain = self.settings.active().then(|| Chain::new(&self.settings, self.rate));
        if let (Some(chain), Some(s)) = (&mut self.chain, saved(self.key)) {
            let v = |i: usize| f64::from_bits(s.state[i].load(Ordering::Relaxed));
            chain.restore_leveler([(v(0), v(1)), (v(2), v(3))]);
        }
        if self.meters.is_none() && self.chain.is_some() {
            self.meters = Map::create(shm::NAME);
        }
    }

    /// Un bloque. Los punteros pueden coincidir (proceso en el sitio): se lee L y R antes de escribir.
    unsafe fn run<T: Sample>(&mut self, inp: [*const T; 2], out: [*mut T; 2], n: usize) {
        let Some(chain) = self.chain.as_mut().filter(|_| !self.failed) else {
            return passthrough(inp, out, n);
        };
        let _ftz = Ftz::on();
        for i in 0..n {
            let (l, r) = chain.tick(inp[0].add(i).read().get(), inp[1].add(i).read().get());
            out[0].add(i).write(T::put(l));
            out[1].add(i).write(T::put(r));
        }
        let m = chain.take_meters();
        if let Some(slot) = self.meters.as_ref().and_then(|map| shm::find(map.slots(), self.key, true)) {
            slot.write(&m);
        }
        if let (Some([(a, b), (c, d)]), Some(s)) = (chain.leveler_state(), saved(self.key)) {
            for (slot, v) in s.state.iter().zip([a, b, c, d]) {
                slot.store(v.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

unsafe fn passthrough<T: Sample>(inp: [*const T; 2], out: [*mut T; 2], n: usize) {
    for c in 0..2 {
        if !std::ptr::eq(inp[c], out[c]) {
            std::ptr::copy(inp[c], out[c], n);
        }
    }
}

/// Cadena C terminada en cero (el host deja 64 bytes; los nombres caben de sobra en 32)
fn text(ptr: *mut c_void, s: &str) -> isize {
    if ptr.is_null() {
        return 0;
    }
    let b = &s.as_bytes()[..s.len().min(31)];
    unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), ptr as *mut u8, b.len());
        *(ptr as *mut u8).add(b.len()) = 0;
    }
    1
}

trait Sample: Copy {
    fn get(self) -> f64;
    fn put(v: f64) -> Self;
}
impl Sample for f32 {
    fn get(self) -> f64 {
        self as f64
    }
    fn put(v: f64) -> Self {
        v as f32
    }
}
impl Sample for f64 {
    fn get(self) -> f64 {
        self
    }
    fn put(v: f64) -> Self {
        v
    }
}

/// Denormales a cero (FTZ + DAZ) mientras dura el bloque; al salir se deja el MXCSR del host
struct Ftz(#[cfg(target_arch = "x86_64")] u32);

impl Ftz {
    #[allow(deprecated)]
    fn on() -> Self {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let old = _mm_getcsr();
            _mm_setcsr(old | 0x8040);
            Ftz(old)
        }
        #[cfg(not(target_arch = "x86_64"))]
        Ftz()
    }
}

impl Drop for Ftz {
    #[allow(deprecated)]
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.0)
        }
    }
}

// --------------------------------------------------------- frontera FFI

/// Punto de entrada que busca el motor. Solo reserva la instancia (si no hay memoria, el proceso se
/// aborta como cualquier otro: no hay panic posible aquí).
#[no_mangle]
pub extern "C" fn VSTPluginMain(_host: HostCallback) -> *mut AEffect {
    Box::into_raw(Plugin::new()) as *mut AEffect
}

extern "C" fn dispatcher(e: *mut AEffect, op: i32, _index: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
    if e.is_null() {
        return 0;
    }
    if op == CLOSE {
        // Si soltar algo fallara, se pierde esa memoria antes que tumbar el proceso
        let _ = catch_unwind(|| drop(unsafe { Box::from_raw(e as *mut Plugin) }));
        return 1;
    }
    let p = unsafe { &mut *(e as *mut Plugin) };
    catch_unwind(AssertUnwindSafe(|| p.dispatch(op, value, ptr, opt))).unwrap_or(0)
}

extern "C" fn process_f32(e: *mut AEffect, inputs: *const *const f32, outputs: *mut *mut f32, frames: i32) {
    process(e, inputs, outputs, frames)
}

extern "C" fn process_f64(e: *mut AEffect, inputs: *const *const f64, outputs: *mut *mut f64, frames: i32) {
    process(e, inputs, outputs, frames)
}

fn process<T: Sample>(e: *mut AEffect, inputs: *const *const T, outputs: *mut *mut T, frames: i32) {
    if e.is_null() || inputs.is_null() || outputs.is_null() || frames <= 0 {
        return;
    }
    unsafe {
        let inp = [*inputs, *inputs.add(1)];
        let out = [*outputs, *outputs.add(1)];
        if inp.iter().any(|p| p.is_null()) || out.iter().any(|p| p.is_null()) {
            return;
        }
        let n = frames as usize;
        let p = &mut *(e as *mut Plugin);
        if catch_unwind(AssertUnwindSafe(|| p.run(inp, out, n))).is_err() {
            p.failed = true;
            passthrough(inp, out, n);
        }
    }
}

extern "C" fn set_parameter(_: *mut AEffect, _: i32, _: f32) {}

extern "C" fn get_parameter(_: *mut AEffect, _: i32) -> f32 {
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: usize = 480;

    struct Host(*mut AEffect);

    impl Host {
        /// Igual que `VSTPluginInstance::initialize` + `prepareForProcessing` + `startProcessing`
        fn load(chunk: Option<&[u8]>) -> Host {
            let e = VSTPluginMain(None);
            let h = Host(e);
            let fx = h.fx();
            assert_eq!(fx.magic, MAGIC);
            assert_eq!((fx.num_inputs, fx.num_outputs), (2, 2));
            assert_ne!(fx.flags & FLAG_REPLACING, 0);
            assert_ne!(fx.flags & FLAG_DOUBLE, 0);
            assert_ne!(fx.flags & FLAG_CHUNKS, 0);
            assert_eq!(fx.initial_delay, 0);
            h.call(OPEN, 0, 0, std::ptr::null_mut(), 0.0);
            h.call(SET_SAMPLE_RATE, 0, 0, std::ptr::null_mut(), 48000.0);
            h.call(11, 0, FRAMES as isize, std::ptr::null_mut(), 0.0);
            if let Some(c) = chunk {
                assert_eq!(h.call(SET_CHUNK, 1, c.len() as isize, c.as_ptr() as *mut c_void, 0.0), 1);
            }
            h.call(MAINS_CHANGED, 0, 1, std::ptr::null_mut(), 0.0);
            h.call(71, 0, 0, std::ptr::null_mut(), 0.0);
            h
        }

        fn fx(&self) -> &AEffect {
            unsafe { &*self.0 }
        }

        fn call(&self, op: i32, index: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
            (self.fx().dispatcher)(self.0, op, index, value, ptr, opt)
        }

        fn run<T: Copy>(&self, f: Process<T>, l: &mut [T], r: &mut [T]) {
            let (il, ir) = (l.to_vec(), r.to_vec());
            let inputs = [il.as_ptr(), ir.as_ptr()];
            let mut outputs = [l.as_mut_ptr(), r.as_mut_ptr()];
            f(self.0, inputs.as_ptr(), outputs.as_mut_ptr(), l.len() as i32);
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            self.call(72, 0, 0, std::ptr::null_mut(), 0.0);
            self.call(MAINS_CHANGED, 0, 0, std::ptr::null_mut(), 0.0);
            assert_eq!(self.call(CLOSE, 0, 0, std::ptr::null_mut(), 0.0), 1);
        }
    }

    fn signal(n: usize, seed: u32) -> Vec<f64> {
        (0..n).map(|i| ((i as u32).wrapping_mul(2654435761).wrapping_add(seed) as f64 / u32::MAX as f64 - 0.5) * 1.8).collect()
    }

    #[test]
    fn todo_apagado_es_bit_transparente() {
        // Sin chunk, con chunk apagado y con un chunk ajeno: la salida es la entrada, bit a bit
        let off = Settings::default().chunk(1);
        for chunk in [None, Some(&off[..]), Some(&b"no es nuestro"[..])] {
            let h = Host::load(chunk);
            let (l, r) = (signal(FRAMES, 1), signal(FRAMES, 2));
            let (mut ol, mut or) = (l.clone(), r.clone());
            h.run(h.fx().process_double_replacing, &mut ol, &mut or);
            assert!(ol.iter().zip(&l).chain(or.iter().zip(&r)).all(|(a, b)| a.to_bits() == b.to_bits()));
            let (l, r): (Vec<f32>, Vec<f32>) = (l.iter().map(|&v| v as f32).collect(), r.iter().map(|&v| v as f32).collect());
            let (mut ol, mut or) = (l.clone(), r.clone());
            h.run(h.fx().process_replacing, &mut ol, &mut or);
            assert!(ol.iter().zip(&l).chain(or.iter().zip(&r)).all(|(a, b)| a.to_bits() == b.to_bits()));
        }
    }

    #[test]
    fn opcodes_del_motor() {
        let mut s = Settings::default();
        s.limiter.on = true;
        s.comp.on = true;
        let chunk = s.chunk(1234);
        let h = Host::load(Some(&chunk));

        // El chunk vuelve igual (lo pide el editor del motor)
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let len = h.call(GET_CHUNK, 1, 0, &mut ptr as *mut _ as *mut c_void, 0.0);
        assert_eq!(unsafe { std::slice::from_raw_parts(ptr, len as usize) }, &chunk[..]);

        let mut name = [0u8; 64];
        assert_eq!(h.call(GET_EFFECT_NAME, 0, 0, name.as_mut_ptr() as _, 0.0), 1);
        assert_eq!(&name[..8], b"PoxiDSP\0");
        assert_eq!(h.call(GET_VST_VERSION, 0, 0, std::ptr::null_mut(), 0.0), 2400);
        // Opcodes que no usamos, punteros nulos y chunks absurdos: 0 y nada roto
        for op in [-1, 2, 13, 14, 25, 33, 51, 999] {
            assert_eq!(h.call(op, 0, 0, std::ptr::null_mut(), 0.0), 0);
        }
        assert_eq!(h.call(SET_CHUNK, 1, 8, std::ptr::null_mut(), 0.0), 0);
        assert_eq!(h.call(GET_EFFECT_NAME, 0, 0, std::ptr::null_mut(), 0.0), 0);

        // Proceso en ambas precisiones, en el sitio, con bloques de varios tamaños y entradas imposibles
        for n in [1, 7, FRAMES, 4096] {
            let mut l: Vec<f64> = signal(n, 3).iter().map(|v| v * 8.0).collect();
            let mut r = signal(n, 4);
            l[0] = f64::NAN;
            h.run(h.fx().process_double_replacing, &mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= 1.0));
            let mut l: Vec<f32> = signal(n, 5).iter().map(|&v| v as f32 * 8.0).collect();
            let mut r = l.clone();
            h.run(h.fx().process_replacing, &mut l, &mut r);
            assert!(l.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        }
        // Punteros nulos y bloques vacíos no hacen nada
        (h.fx().process_replacing)(h.0, std::ptr::null(), std::ptr::null_mut(), 64);
        let mut z = [0f32; 4];
        let (zi, mut zo) = ([z.as_ptr(), std::ptr::null()], [z.as_mut_ptr(), z.as_mut_ptr()]);
        (h.fx().process_replacing)(h.0, zi.as_ptr(), zo.as_mut_ptr(), 4);
        (h.fx().process_replacing)(h.0, zi.as_ptr(), zo.as_mut_ptr(), -1);
        z[0] = 1.0;
        assert_eq!(z[0], 1.0);
    }

    #[test]
    fn proceso_antes_de_arrancar_es_paso_directo() {
        let e = VSTPluginMain(None);
        let h = Host(e);
        let (l, r) = (signal(64, 9), signal(64, 10));
        let (mut ol, mut or) = (l.clone(), r.clone());
        h.run(h.fx().process_double_replacing, &mut ol, &mut or);
        assert_eq!((ol, or), (l, r));
    }
}
