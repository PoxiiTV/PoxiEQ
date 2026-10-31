//! Medidas plugin → app en memoria compartida con nombre.
//!
//! El plugin (audiodg.exe, LOCAL SERVICE, sesión 0) crea `Global\PoxiDSP` con una DACL que deja leer a
//! todos; la app (sesión del usuario) lo abre solo para leer. Un hueco por dispositivo, identificado
//! con [`crate::device_key`]. Todo son atómicos: ni bloqueos ni esperas en el hilo de audio.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

pub const NAME: &str = "Global\\PoxiDSP";
pub const SLOTS: usize = 8;

#[repr(C)]
#[derive(Default)]
pub struct Slot {
    /// Dispositivo (0 = libre)
    pub key: AtomicU32,
    /// Sube en cada bloque procesado: si no se mueve, el plugin no está sonando
    pub beat: AtomicU32,
    /// f32 en bits: reducción del compresor y del limitador (dB ≥ 0) y pico de salida (dBFS)
    pub gr_comp: AtomicU32,
    pub gr_limit: AtomicU32,
    pub peak: AtomicU32,
}

#[repr(C)]
pub struct Block {
    pub slots: [Slot; SLOTS],
}

/// Lectura de un hueco
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub beat: u32,
    pub gr_comp: f32,
    pub gr_limit: f32,
    pub peak: f32,
}

impl Slot {
    pub fn write(&self, m: &crate::dsp::Meters) {
        self.gr_comp.store(m.gr_comp.to_bits(), Ordering::Relaxed);
        self.gr_limit.store(m.gr_limit.to_bits(), Ordering::Relaxed);
        self.peak.store(m.peak.to_bits(), Ordering::Relaxed);
        self.beat.fetch_add(1, Ordering::Release);
    }

    pub fn read(&self) -> Reading {
        let f = |a: &AtomicU32| {
            let v = f32::from_bits(a.load(Ordering::Relaxed));
            if v.is_finite() { v } else { 0.0 }
        };
        Reading { beat: self.beat.load(Ordering::Acquire), gr_comp: f(&self.gr_comp).clamp(0.0, 96.0), gr_limit: f(&self.gr_limit).clamp(0.0, 96.0), peak: f(&self.peak).clamp(-240.0, 96.0) }
    }
}

/// Busca el hueco de `key` o, si `claim`, ocupa uno libre (sin bloqueos: compare-exchange)
pub fn find(slots: &[Slot], key: u32, claim: bool) -> Option<&Slot> {
    find_by(slots, |s| &s.key, key, claim)
}

/// [`find`] para cualquier tabla con una clave atómica por hueco
pub fn find_by<T>(items: &[T], key_of: fn(&T) -> &AtomicU32, key: u32, claim: bool) -> Option<&T> {
    if key == 0 {
        return None;
    }
    if let Some(s) = items.iter().find(|s| key_of(s).load(Ordering::Acquire) == key) {
        return Some(s);
    }
    if !claim {
        return None;
    }
    items.iter().find(|s| match key_of(s).compare_exchange(0, key, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => true,
        Err(k) => k == key,
    })
}

// ------------------------------------------------------------------ Win32

type Handle = *mut c_void;

#[repr(C)]
struct SecurityAttributes {
    len: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

const PAGE_READWRITE: u32 = 0x04;
const FILE_MAP_READ: u32 = 0x04;
const FILE_MAP_WRITE: u32 = 0x02;
const SDDL_REVISION_1: u32 = 1;

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileMappingW(file: Handle, sa: *const SecurityAttributes, protect: u32, high: u32, low: u32, name: *const u16) -> Handle;
    fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn MapViewOfFile(map: Handle, access: u32, high: u32, low: u32, bytes: usize) -> *mut c_void;
    fn UnmapViewOfFile(addr: *const c_void) -> i32;
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

/// Vista de la memoria compartida; se suelta al soltar el valor
pub struct Map {
    handle: Handle,
    view: *mut Block,
}

// La vista solo se toca con atómicos
unsafe impl Send for Map {}
unsafe impl Sync for Map {}

impl Map {
    /// Lado del plugin: crea (o abre, si ya existe) con lectura para todos y escritura solo para
    /// sistema, servicio local y administradores
    pub fn create(name: &str) -> Option<Map> {
        let sddl = wide("D:(A;;GA;;;SY)(A;;GA;;;LS)(A;;GA;;;BA)(A;;GA;;;OW)(A;;GR;;;WD)");
        let name = wide(name);
        unsafe {
            let mut sd = std::ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) == 0 {
                return None;
            }
            let sa = SecurityAttributes { len: size_of::<SecurityAttributes>() as u32, descriptor: sd, inherit: 0 };
            let handle = CreateFileMappingW(-1isize as Handle, &sa, PAGE_READWRITE, 0, size_of::<Block>() as u32, name.as_ptr());
            LocalFree(sd);
            Self::view(handle, FILE_MAP_READ | FILE_MAP_WRITE)
        }
    }

    /// Lado de la app: solo lectura. `None` si el plugin no ha arrancado
    pub fn open(name: &str) -> Option<Map> {
        let name = wide(name);
        unsafe { Self::view(OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr()), FILE_MAP_READ) }
    }

    unsafe fn view(handle: Handle, access: u32) -> Option<Map> {
        if handle.is_null() {
            return None;
        }
        let view = MapViewOfFile(handle, access, 0, 0, size_of::<Block>()) as *mut Block;
        if view.is_null() {
            CloseHandle(handle);
            return None;
        }
        Some(Map { handle, view })
    }

    pub fn slots(&self) -> &[Slot] {
        // La vista vive lo que `self` y empieza a ceros (todos los huecos libres)
        unsafe { &(*self.view).slots }
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view as _);
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::Meters;

    #[test]
    fn plugin_escribe_y_app_lee() {
        // En los tests no hay permiso para `Global\`: mismo código con un nombre de la sesión
        let name = format!("Local\\PoxiDSPTest{}", std::process::id());
        assert!(Map::open(&name).is_none());
        let plugin = Map::create(&name).expect("crear");
        let slot = find(plugin.slots(), 42, true).unwrap();
        assert!(std::ptr::eq(slot, find(plugin.slots(), 42, true).unwrap()), "el mismo dispositivo, el mismo hueco");
        slot.write(&Meters { gr_comp: 3.5, gr_limit: 1.25, peak: -1.0 });

        let app = Map::open(&name).expect("abrir");
        assert!(find(app.slots(), 7, false).is_none());
        let r = find(app.slots(), 42, false).unwrap().read();
        assert_eq!(r, Reading { beat: 1, gr_comp: 3.5, gr_limit: 1.25, peak: -1.0 });

        // Huecos llenos: sin medidor, sin fallo
        for k in 100..100 + SLOTS as u32 {
            find(plugin.slots(), k, true);
        }
        assert!(find(plugin.slots(), 999, true).is_none());
    }
}
