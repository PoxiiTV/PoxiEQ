//! VST3 alojado (ver `host.rs`). Con el crate `vst3` (bindings generados de las cabeceras del SDK, MIT).
//!
//! En el audio solo hace falta el componente (el procesador). Para editar se carga también el controlador, se
//! conectan los dos y los cambios del editor (`performEdit`) se le pasan al procesador en un bloque en silencio
//! antes de leer su estado: así el estado que va al motor ya lleva lo que se tocó en la ventana.

use crate::host::{c_text, vst3_binary, Format, Info, Library};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::sync::{Arc, Mutex};
use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

const AUDIO_EFFECT: &str = "Audio Module Class";

fn ok(r: tresult) -> bool {
    r == kResultOk
}

struct Module {
    factory: ComPtr<IPluginFactory>,
    lib: Library,
}

impl Module {
    fn load(path: &str) -> Result<Module, String> {
        let lib = Library::load(&vst3_binary(path))?;
        let init = lib.symbol(b"InitDll\0");
        if !init.is_null() {
            let init: extern "system" fn() -> bool = unsafe { std::mem::transmute(init) };
            init();
        }
        let get = lib.symbol(b"GetPluginFactory\0");
        if get.is_null() {
            return Err("no es un plugin VST3".into());
        }
        let get: extern "system" fn() -> *mut IPluginFactory = unsafe { std::mem::transmute(get) };
        let factory = unsafe { ComPtr::from_raw(get()) }.ok_or("el plugin no da su factoría")?;
        Ok(Module { factory, lib })
    }

    fn create<I: Interface>(&self, cid: &TUID) -> Option<ComPtr<I>> {
        let mut obj: *mut c_void = std::ptr::null_mut();
        let r = unsafe { self.factory.createInstance(cid.as_ptr(), I::IID.as_ptr() as FIDString, &mut obj) };
        if ok(r) {
            unsafe { ComPtr::from_raw(obj as *mut I) }
        } else {
            None
        }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        let exit = self.lib.symbol(b"ExitDll\0");
        if !exit.is_null() {
            let exit: extern "system" fn() -> bool = unsafe { std::mem::transmute(exit) };
            exit();
        }
    }
}

pub fn scan(path: &str) -> Result<Vec<Info>, String> {
    let m = Module::load(path)?;
    let f = &m.factory;
    let mut finfo: PFactoryInfo = unsafe { std::mem::zeroed() };
    unsafe { f.getFactoryInfo(&mut finfo) };
    let vendor = c_text(&finfo.vendor);
    let f2 = f.cast::<IPluginFactory2>();
    let mut out = vec![];
    for i in 0..unsafe { f.countClasses() } {
        let mut ci: PClassInfo = unsafe { std::mem::zeroed() };
        if !ok(unsafe { f.getClassInfo(i, &mut ci) }) || c_text(&ci.category) != AUDIO_EFFECT {
            continue;
        }
        let mut vendor = vendor.clone();
        if let Some(f2) = &f2 {
            let mut c2: PClassInfo2 = unsafe { std::mem::zeroed() };
            if ok(unsafe { f2.getClassInfo2(i, &mut c2) }) {
                if c_text(&c2.subCategories).contains("Instrument") {
                    continue;
                }
                let v = c_text(&c2.vendor);
                if !v.is_empty() {
                    vendor = v;
                }
            }
        }
        let cid: [u8; 16] = unsafe { std::mem::transmute(ci.cid) };
        // La latencia se sabe con el procesador preparado
        let latency = Vst3::load(path, &cid, &[], 48000.0, 512, false).map(|p| p.latency()).unwrap_or(0);
        out.push(Info { format: Format::Vst3, path: path.into(), cid, name: c_text(&ci.name), vendor, latency });
    }
    if out.is_empty() {
        return Err("no trae ningún efecto".into());
    }
    Ok(out)
}

// ------------------------------------------------------------- instancia

pub struct Vst3 {
    comp: ComPtr<IComponent>,
    proc: ComPtr<IAudioProcessor>,
    ctrl: Option<ComPtr<IEditController>>,
    /// Componente y controlador conectados (si son objetos distintos)
    links: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    view: Option<ComPtr<IPlugView>>,
    frame: Option<ComWrapper<Frame>>,
    edits: Arc<Mutex<Vec<(ParamID, f64)>>>,
    _handler: Option<ComWrapper<Handler>>,
    _host: ComWrapper<Host>,
    /// Búferes de cada bus de entrada y salida (el principal, estéreo; los demás, a cero)
    ins: Vec<Vec<Vec<f32>>>,
    outs: Vec<Vec<Vec<f32>>>,
    ctx: ProcessContext,
    rate: f64,
    block: usize,
    _module: Module,
}

unsafe impl Send for Vst3 {}

impl Vst3 {
    pub(crate) fn load(path: &str, cid: &[u8; 16], state: &[u8], rate: f64, block: usize, editor: bool) -> Result<Box<Vst3>, String> {
        let module = Module::load(path)?;
        let tuid: TUID = unsafe { std::mem::transmute(*cid) };
        let host = ComWrapper::new(Host);
        let host_ptr = host.to_com_ptr::<FUnknown>().ok_or("host")?.as_ptr();
        let comp: ComPtr<IComponent> = module.create(&tuid).ok_or("no se puede crear el plugin")?;
        if !ok(unsafe { comp.initialize(host_ptr) }) {
            return Err("el plugin no arranca".into());
        }
        let proc = comp.cast::<IAudioProcessor>().ok_or("no procesa audio")?;
        let (mut ctrl, mut links, mut handler) = (None, None, None);
        let edits = Arc::new(Mutex::new(vec![]));
        if editor {
            ctrl = comp.cast::<IEditController>();
            if ctrl.is_none() {
                let mut ccid: TUID = [0; 16];
                if ok(unsafe { comp.getControllerClassId(&mut ccid) }) {
                    ctrl = module.create::<IEditController>(&ccid).filter(|c| ok(unsafe { c.initialize(host_ptr) }));
                    if let (Some(a), Some(b)) = (comp.cast::<IConnectionPoint>(), ctrl.as_ref().and_then(|c| c.cast::<IConnectionPoint>())) {
                        unsafe {
                            a.connect(b.as_ptr());
                            b.connect(a.as_ptr());
                        }
                        links = Some((a, b));
                    }
                }
            }
            if let Some(c) = &ctrl {
                let h = ComWrapper::new(Handler(edits.clone()));
                if let Some(hp) = h.to_com_ptr::<IComponentHandler>() {
                    unsafe { c.setComponentHandler(hp.as_ptr()) };
                }
                handler = Some(h);
            }
        }

        // Buses: el principal en estéreo, los demás (sidechain…) apagados pero con búferes por si acaso
        let mut stereo = [SpeakerArr::kStereo];
        let mut stereo_out = [SpeakerArr::kStereo];
        unsafe {
            proc.setBusArrangements(stereo.as_mut_ptr(), 1, stereo_out.as_mut_ptr(), 1);
            comp.activateBus(MediaTypes_::kAudio, BusDirections_::kInput, 0, 1);
            comp.activateBus(MediaTypes_::kAudio, BusDirections_::kOutput, 0, 1);
        }
        let buses = |dir: i32| -> Vec<Vec<Vec<f32>>> {
            let n = unsafe { comp.getBusCount(MediaTypes_::kAudio, dir) }.max(0);
            (0..n)
                .map(|i| {
                    let mut info: BusInfo = unsafe { std::mem::zeroed() };
                    unsafe { comp.getBusInfo(MediaTypes_::kAudio, dir, i, &mut info) };
                    let ch = if i == 0 { 2 } else { info.channelCount.clamp(0, 8) as usize };
                    vec![vec![0.0; block]; ch]
                })
                .collect()
        };
        let (ins, outs) = (buses(BusDirections_::kInput), buses(BusDirections_::kOutput));
        if outs.is_empty() {
            return Err("el plugin no tiene salida de audio".into());
        }
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32,
            maxSamplesPerBlock: block as i32,
            sampleRate: rate,
        };
        if !ok(unsafe { proc.setupProcessing(&mut setup) }) {
            return Err("el plugin no acepta esta frecuencia".into());
        }

        let mut ctx: ProcessContext = unsafe { std::mem::zeroed() };
        ctx.sampleRate = rate;
        ctx.tempo = 120.0;
        ctx.timeSigNumerator = 4;
        ctx.timeSigDenominator = 4;
        ctx.state = ProcessContext_::StatesAndFlags_::kTempoValid as u32;
        let mut p = Box::new(Vst3 { comp, proc, ctrl, links, view: None, frame: None, edits, _handler: handler, _host: host, ins, outs, ctx, rate, block, _module: module });
        p.restore(state);
        unsafe {
            p.comp.setActive(1);
            p.proc.setProcessing(1);
        }
        Ok(p)
    }

    /// Estado: largo + estado del componente, largo + estado del controlador
    fn restore(&mut self, state: &[u8]) {
        let (comp, ctrl) = split_state(state);
        if comp.is_empty() {
            return;
        }
        unsafe {
            self.comp.setState(Stream::new(comp).as_ptr());
            if let Some(c) = &self.ctrl {
                c.setComponentState(Stream::new(comp).as_ptr());
                if !ctrl.is_empty() {
                    c.setState(Stream::new(ctrl).as_ptr());
                }
            }
        }
    }

    pub(crate) fn state(&mut self) -> Vec<u8> {
        let comp = Stream::new(&[]);
        unsafe { self.comp.getState(comp.as_ptr()) };
        let ctrl = Stream::new(&[]);
        if let Some(c) = &self.ctrl {
            unsafe { c.getState(ctrl.as_ptr()) };
        }
        join_state(&comp.bytes(), &ctrl.bytes())
    }

    /// Lo tocado en el editor llega al procesador en un bloque en silencio (solo con el audio parado: si no, ya
    /// le llega en cada bloque)
    pub(crate) fn flush_edits(&mut self) {
        let edits = std::mem::take(&mut *self.edits.lock().unwrap_or_else(|e| e.into_inner()));
        if edits.is_empty() {
            return;
        }
        let changes = ComWrapper::new(Changes(edits.into_iter().map(|(id, v)| ComWrapper::new(Queue { id, value: v })).collect()));
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        self.run(&mut l, &mut r, changes.to_com_ptr::<IParameterChanges>().map_or(std::ptr::null_mut(), |c| c.as_ptr()));
    }

    pub(crate) fn latency(&self) -> u32 {
        unsafe { self.proc.getLatencySamples() }
    }

    /// Un bloque, con lo que se haya tocado en la ventana desde el anterior
    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let edits = self.edits.try_lock().map(|mut e| std::mem::take(&mut *e)).unwrap_or_default();
        if edits.is_empty() {
            return self.run(l, r, std::ptr::null_mut());
        }
        let changes = ComWrapper::new(Changes(edits.into_iter().map(|(id, v)| ComWrapper::new(Queue { id, value: v })).collect()));
        self.run(l, r, changes.to_com_ptr::<IParameterChanges>().map_or(std::ptr::null_mut(), |c| c.as_ptr()));
    }

    pub(crate) fn set_rate(&mut self, rate: f64) {
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32,
            maxSamplesPerBlock: self.block as i32,
            sampleRate: rate,
        };
        unsafe {
            self.proc.setProcessing(0);
            self.comp.setActive(0);
            self.proc.setupProcessing(&mut setup);
            self.comp.setActive(1);
            self.proc.setProcessing(1);
        }
        self.rate = rate;
        self.ctx.sampleRate = rate;
    }

    fn run(&mut self, l: &mut [f32], r: &mut [f32], changes: *mut IParameterChanges) {
        let n = l.len().min(r.len()).min(self.block);
        if let Some(main) = self.ins.first_mut() {
            main[0][..n].copy_from_slice(&l[..n]);
            main[1][..n].copy_from_slice(&r[..n]);
        }
        // ponytail: punteros por bloque en Vec; guardarlos en la instancia si alguna vez pesa
        let mut in_ptrs: Vec<Vec<*mut f32>> = self.ins.iter_mut().map(|b| b.iter_mut().map(|c| c.as_mut_ptr()).collect()).collect();
        let mut out_ptrs: Vec<Vec<*mut f32>> = self.outs.iter_mut().map(|b| b.iter_mut().map(|c| c.as_mut_ptr()).collect()).collect();
        let bus = |ptrs: &mut Vec<*mut f32>| AudioBusBuffers {
            numChannels: ptrs.len() as i32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 { channelBuffers32: ptrs.as_mut_ptr() },
        };
        let mut ins: Vec<AudioBusBuffers> = in_ptrs.iter_mut().map(bus).collect();
        let mut outs: Vec<AudioBusBuffers> = out_ptrs.iter_mut().map(bus).collect();
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32,
            numSamples: n as i32,
            numInputs: ins.len() as i32,
            numOutputs: outs.len() as i32,
            inputs: if ins.is_empty() { std::ptr::null_mut() } else { ins.as_mut_ptr() },
            outputs: outs.as_mut_ptr(),
            inputParameterChanges: changes,
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: std::ptr::null_mut(),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut self.ctx,
        };
        if ok(unsafe { self.proc.process(&mut data) }) {
            l[..n].copy_from_slice(&self.outs[0][0][..n]);
            r[..n].copy_from_slice(&self.outs[0][1.min(self.outs[0].len() - 1)][..n]);
        }
        self.ctx.continousTimeSamples += n as i64;
    }

    fn create_view(&mut self) -> Option<&ComPtr<IPlugView>> {
        if self.view.is_none() {
            let c = self.ctrl.as_ref()?;
            let v = unsafe { c.createView(ViewType::kEditor) };
            self.view = unsafe { ComPtr::from_raw(v) };
        }
        self.view.as_ref()
    }

    pub(crate) fn editor_size(&mut self) -> Option<(i32, i32)> {
        let v = self.create_view()?;
        let mut r = ViewRect { left: 0, top: 0, right: 0, bottom: 0 };
        ok(unsafe { v.getSize(&mut r) }).then_some((r.right - r.left, r.bottom - r.top)).filter(|&(w, h)| w > 0 && h > 0)
    }

    pub(crate) fn open_editor(&mut self, hwnd: *mut c_void, resize: Box<dyn Fn(i32, i32)>) -> bool {
        let frame = ComWrapper::new(Frame(resize));
        let Some(v) = self.create_view().cloned() else { return false };
        unsafe {
            if !ok(v.isPlatformTypeSupported(kPlatformTypeHWND)) {
                return false;
            }
            if let Some(f) = frame.to_com_ptr::<IPlugFrame>() {
                v.setFrame(f.as_ptr());
            }
            if !ok(v.attached(hwnd, kPlatformTypeHWND)) {
                return false;
            }
        }
        self.frame = Some(frame);
        true
    }

    pub(crate) fn close_editor(&mut self) {
        if let Some(v) = self.view.take() {
            unsafe {
                v.removed();
                v.setFrame(std::ptr::null_mut());
            }
        }
        self.frame = None;
    }

}

impl Drop for Vst3 {
    fn drop(&mut self) {
        self.close_editor();
        unsafe {
            self.proc.setProcessing(0);
            self.comp.setActive(0);
            if let Some((a, b)) = self.links.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = self.ctrl.take() {
                c.setComponentHandler(std::ptr::null_mut());
                // Si es el mismo objeto que el componente, ya lo termina `comp`
                if self.comp.cast::<IEditController>().is_none() {
                    c.terminate();
                }
            }
            self.comp.terminate();
        }
    }
}

fn join_state(comp: &[u8], ctrl: &[u8]) -> Vec<u8> {
    let mut s = Vec::with_capacity(8 + comp.len() + ctrl.len());
    s.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    s.extend_from_slice(comp);
    s.extend_from_slice(&(ctrl.len() as u32).to_le_bytes());
    s.extend_from_slice(ctrl);
    s
}

fn split_state(s: &[u8]) -> (&[u8], &[u8]) {
    fn part(s: &[u8]) -> Option<(&[u8], usize)> {
        let len = u32::from_le_bytes(s.get(..4)?.try_into().ok()?) as usize;
        Some((s.get(4..4 + len)?, 4 + len))
    }
    let Some((comp, used)) = part(s) else { return (&[], &[]) };
    let ctrl = part(&s[used..]).map_or(&[][..], |(c, _)| c);
    (comp, ctrl)
}

// --------------------------------------------------------- clases del host

/// `IBStream` en memoria (para leer y escribir estados)
struct Stream(Mutex<(Vec<u8>, usize)>);

impl Stream {
    fn new(data: &[u8]) -> ComWrapper<Stream> {
        ComWrapper::new(Stream(Mutex::new((data.to_vec(), 0))))
    }
}

trait StreamPtr {
    fn as_ptr(&self) -> *mut IBStream;
    fn bytes(&self) -> Vec<u8>;
}

impl StreamPtr for ComWrapper<Stream> {
    fn as_ptr(&self) -> *mut IBStream {
        // El ComPtr temporal se suelta, pero el objeto sigue vivo mientras viva este ComWrapper
        self.to_com_ptr::<IBStream>().map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().map(|g| g.0.clone()).unwrap_or_default()
    }
}

impl Class for Stream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for Stream {
    unsafe fn read(&self, buffer: *mut c_void, n: int32, read: *mut int32) -> tresult {
        let Ok(mut g) = self.0.lock() else { return kResultFalse };
        let (data, pos) = &mut *g;
        let take = (n.max(0) as usize).min(data.len().saturating_sub(*pos));
        if take > 0 && !buffer.is_null() {
            std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer as *mut u8, take);
        }
        *pos += take;
        if !read.is_null() {
            *read = take as i32;
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, n: int32, written: *mut int32) -> tresult {
        let Ok(mut g) = self.0.lock() else { return kResultFalse };
        let (data, pos) = &mut *g;
        let n = n.max(0) as usize;
        if buffer.is_null() || *pos + n > crate::host::MAX_STATE {
            return kResultFalse;
        }
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        std::ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(*pos), n);
        *pos += n;
        if !written.is_null() {
            *written = n as i32;
        }
        kResultOk
    }

    unsafe fn seek(&self, to: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut g) = self.0.lock() else { return kResultFalse };
        let (data, pos) = &mut *g;
        let base = match mode as u32 {
            m if m == IBStream_::IStreamSeekMode_::kIBSeekSet as u32 => 0,
            m if m == IBStream_::IStreamSeekMode_::kIBSeekCur as u32 => *pos as i64,
            _ => data.len() as i64,
        };
        *pos = (base + to).clamp(0, data.len() as i64) as usize;
        if !result.is_null() {
            *result = *pos as i64;
        }
        kResultOk
    }

    unsafe fn tell(&self, out: *mut int64) -> tresult {
        let Ok(g) = self.0.lock() else { return kResultFalse };
        if !out.is_null() {
            *out = g.1 as i64;
        }
        kResultOk
    }
}

/// `IHostApplication`: nombre y los mensajes con los que hablan componente y controlador
struct Host;

impl Class for Host {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for Host {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if let Some(n) = name.as_mut() {
            for (d, s) in n.iter_mut().zip("PoxiEQ".encode_utf16().chain([0])) {
                *d = s;
            }
        }
        kResultOk
    }

    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        let as_guid = |t: *mut TUID| -> [u8; 16] { std::mem::transmute(*t) };
        let (c, i) = (as_guid(cid), as_guid(iid));
        let ptr = if c == IMessage::IID && i == IMessage::IID {
            ComWrapper::new(Message::default()).to_com_ptr::<IMessage>().map(|p| p.into_raw() as *mut c_void)
        } else if c == IAttributeList::IID && i == IAttributeList::IID {
            ComWrapper::new(Attributes::default()).to_com_ptr::<IAttributeList>().map(|p| p.into_raw() as *mut c_void)
        } else {
            None
        };
        match ptr {
            Some(p) => {
                *obj = p;
                kResultOk
            }
            None => {
                *obj = std::ptr::null_mut();
                kResultFalse
            }
        }
    }
}

#[derive(Default)]
struct Message {
    id: Mutex<Option<std::ffi::CString>>,
    attrs: std::sync::OnceLock<ComWrapper<Attributes>>,
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().ok().and_then(|g| g.as_ref().map(|s| s.as_ptr())).unwrap_or(std::ptr::null())
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        if let Ok(mut g) = self.id.lock() {
            *g = (!id.is_null()).then(|| CStr::from_ptr(id).to_owned());
        }
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        let a = self.attrs.get_or_init(|| ComWrapper::new(Attributes::default()));
        a.to_com_ptr::<IAttributeList>().map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }
}

enum Attr {
    Int(i64),
    Float(f64),
    Text(Vec<u16>),
    Binary(Vec<u8>),
}

#[derive(Default)]
struct Attributes(Mutex<HashMap<Vec<u8>, Attr>>);

impl Attributes {
    unsafe fn key(id: AttrID) -> Vec<u8> {
        if id.is_null() {
            vec![]
        } else {
            CStr::from_ptr(id as *const c_char).to_bytes().to_vec()
        }
    }
    fn set(&self, id: AttrID, v: Attr) -> tresult {
        match self.0.lock() {
            Ok(mut m) => {
                m.insert(unsafe { Self::key(id) }, v);
                kResultOk
            }
            Err(_) => kResultFalse,
        }
    }
}

impl Class for Attributes {
    type Interfaces = (IAttributeList,);
}

impl IAttributeListTrait for Attributes {
    unsafe fn setInt(&self, id: AttrID, value: int64) -> tresult {
        self.set(id, Attr::Int(value))
    }
    unsafe fn getInt(&self, id: AttrID, value: *mut int64) -> tresult {
        match (self.0.lock().ok().as_ref().and_then(|m| match m.get(&Self::key(id)) { Some(Attr::Int(v)) => Some(*v), _ => None }), value.as_mut()) {
            (Some(v), Some(out)) => {
                *out = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Attr::Float(value))
    }
    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        match (self.0.lock().ok().as_ref().and_then(|m| match m.get(&Self::key(id)) { Some(Attr::Float(v)) => Some(*v), _ => None }), value.as_mut()) {
            (Some(v), Some(out)) => {
                *out = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: AttrID, s: *const TChar) -> tresult {
        if s.is_null() {
            return kInvalidArgument;
        }
        let mut v = vec![];
        let mut i = 0;
        while *s.add(i) != 0 && i < 1 << 16 {
            v.push(*s.add(i));
            i += 1;
        }
        self.set(id, Attr::Text(v))
    }
    unsafe fn getString(&self, id: AttrID, out: *mut TChar, size: uint32) -> tresult {
        let Ok(m) = self.0.lock() else { return kResultFalse };
        let Some(Attr::Text(v)) = m.get(&Self::key(id)) else { return kResultFalse };
        let cap = (size as usize / 2).saturating_sub(1);
        if out.is_null() || cap == 0 && !v.is_empty() {
            return kResultFalse;
        }
        let n = v.len().min(cap);
        std::ptr::copy_nonoverlapping(v.as_ptr(), out, n);
        *out.add(n) = 0;
        kResultOk
    }
    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size: uint32) -> tresult {
        if data.is_null() && size > 0 {
            return kInvalidArgument;
        }
        let v = if size == 0 { vec![] } else { std::slice::from_raw_parts(data as *const u8, size as usize).to_vec() };
        self.set(id, Attr::Binary(v))
    }
    unsafe fn getBinary(&self, id: AttrID, data: *mut *const c_void, size: *mut uint32) -> tresult {
        let Ok(m) = self.0.lock() else { return kResultFalse };
        let Some(Attr::Binary(v)) = m.get(&Self::key(id)) else { return kResultFalse };
        if data.is_null() || size.is_null() {
            return kInvalidArgument;
        }
        // El puntero vale mientras viva la lista (como en el SDK)
        *data = v.as_ptr() as *const c_void;
        *size = v.len() as u32;
        kResultOk
    }
}

/// Recoge lo que se toca en el editor
struct Handler(Arc<Mutex<Vec<(ParamID, f64)>>>);

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if let Ok(mut e) = self.0.lock() {
            match e.iter_mut().find(|(i, _)| *i == id) {
                Some(slot) => slot.1 = value,
                None => e.push((id, value)),
            }
        }
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        kResultOk
    }
}

/// La ventana que aloja el editor: le cambia el tamaño cuando el plugin lo pide
struct Frame(Box<dyn Fn(i32, i32)>);

impl Class for Frame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        let (Some(r), Some(v)) = (size.as_mut(), vst3::ComRef::<IPlugView>::from_raw(view)) else { return kInvalidArgument };
        (self.0)(r.right - r.left, r.bottom - r.top);
        v.onSize(r);
        kResultOk
    }
}

/// Cambios de parámetros para un bloque (un punto por parámetro, al principio)
struct Changes(Vec<ComWrapper<Queue>>);

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.0.len() as i32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        self.0.get(index as usize).and_then(|q| q.to_com_ptr::<IParamValueQueue>()).map_or(std::ptr::null_mut(), |p| p.as_ptr())
    }
    unsafe fn addParameterData(&self, _id: *const ParamID, _index: *mut int32) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

struct Queue {
    id: ParamID,
    value: f64,
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }
    unsafe fn getPointCount(&self) -> int32 {
        1
    }
    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        if index != 0 || offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        *offset = 0;
        *value = self.value;
        kResultOk
    }
    unsafe fn addPoint(&self, _offset: int32, _value: ParamValue, _index: *mut int32) -> tresult {
        kResultFalse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estado_en_dos_partes() {
        let s = join_state(&[1, 2, 3], &[9]);
        assert_eq!(split_state(&s), (&[1u8, 2, 3][..], &[9u8][..]));
        assert_eq!(split_state(&[]), (&[][..], &[][..]));
        assert_eq!(split_state(&[5, 0, 0, 0, 1]), (&[][..], &[][..]));
    }
}
