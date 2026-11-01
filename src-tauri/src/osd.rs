//! Aviso en pantalla al cambiar de preset o de bypass con un atajo: una ventanita transparente,
//! siempre encima, que no roba el foco (se enseña sin activarse) ni sale en la barra de tareas.
//! La animación la hace la página (`osd.html`); aquí solo se coloca, se enseña y se esconde.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE};

/// Tamaño lógico de la ventana (el aviso va centrado dentro, con margen para la sombra y la animación)
const W: f64 = 380.0;
const H: f64 = 120.0;
/// Separación desde el borde inferior del área de trabajo (encima de la barra de tareas)
const BOTTOM: f64 = 64.0;
/// Lo que se queda a la vista (la página anima la salida en 500 ms; luego se esconde la ventana)
pub const SHORT: u64 = 1200;
/// Avisos que hay que leer (el motor dejó de funcionar)
pub const LONG: u64 = 6000;

/// Cada aviso sube el contador: solo esconde la ventana el último
static SHOWN: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
struct Payload<'a> {
    title: &'a str,
    sub: &'a str,
    /// `preset`, `on`, `off` o `warn` (icono)
    kind: &'a str,
    ms: u64,
    theme: &'a str,
    accent: &'a str,
}

/// Que los clics la atraviesen: nunca se interpone con lo que haya debajo
pub fn init(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("osd") {
        let _ = w.set_ignore_cursor_events(true);
    }
}

pub fn show(app: &AppHandle, title: &str, sub: &str, kind: &str, ms: u64) {
    let Some(win) = app.get_webview_window("osd") else { return };
    let st = app.state::<crate::AppState>();
    let (theme, accent) = {
        let c = st.cfg();
        (c.theme.clone(), c.accent.clone())
    };
    // JSON = literal de JS válido y escapado: el nombre del preset no puede romper nada
    let payload = serde_json::to_string(&Payload { title, sub, kind, ms, theme: &theme, accent: &accent }).unwrap_or_default();
    let _ = win.eval(format!("window.osd && window.osd({payload})"));

    // En la pantalla del ratón (la del juego o la app que se esté usando), abajo en el centro
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(hwnd)) = (monitor, win.hwnd()) else { return };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (w, h) = ((W * scale).round() as i32, (H * scale).round() as i32);
    let x = area.position.x + (area.size.width as i32 - w) / 2;
    let y = area.position.y + area.size.height as i32 - h - (BOTTOM * scale).round() as i32;
    // Directo a Win32: `show()` de Tauri activaría la ventana y le quitaría el foco al juego
    unsafe {
        let _ = SetWindowPos(HWND(hwnd.0), Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    let id = SHOWN.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms + 500));
        if SHOWN.load(Ordering::SeqCst) == id {
            if let Ok(hwnd) = win.hwnd() {
                unsafe {
                    let _ = ShowWindow(HWND(hwnd.0), SW_HIDE);
                }
            }
        }
    });
}
