//! A janela do controle: aparece colada na bandeja e some ao perder o foco.

use crate::winutil;
use slint::winit_030::winit;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;

/// Tamanho do popup (pixels lógicos). A altura diminui em telas baixas e o conteúdo rola.
const WIDTH: f32 = 380.0;
const HEIGHT: f32 = 690.0;
/// Distância das bordas da área útil (lógica).
const MARGIN: f64 = 12.0;

/// HWND da janela, se ela já existir.
pub fn hwnd(window: &slint::Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window
        .with_winit_window(|w: &winit::window::Window| match w.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        })
        .flatten()
}

/// Mostra o popup ao lado do ponto (o ícone da bandeja) ou no canto da área útil.
pub fn show(component: &impl ComponentHandle, anchor: Option<(i32, i32)>) {
    let window = component.window();
    let Some(wa) = winutil::work_area(anchor).or_else(|| winutil::work_area(None)) else {
        let _ = component.show();
        return;
    };
    let s = wa.scale;
    let m = MARGIN * s;
    let w = (WIDTH as f64 * s).round();
    let h = (HEIGHT as f64 * s).min(wa.h as f64 - 2.0 * m).max(300.0 * s).round();
    window.set_size(slint::PhysicalSize::new(w as u32, h as u32));

    let (wx, wy, ww, wh) = (wa.x as f64, wa.y as f64, wa.w as f64, wa.h as f64);
    let (px, py) = anchor
        .map(|(x, y)| (x as f64, y as f64))
        .unwrap_or((wx + ww, wy + wh + 1.0));
    // f64::clamp dá panic se min > max (tela menor que o popup); aqui o mínimo vence.
    let clamp = |v: f64, min: f64, max: f64| v.min(max).max(min);
    let clamp_x = |x: f64| clamp(x, wx + m, wx + ww - w - m);
    let clamp_y = |y: f64| clamp(y, wy + m, wy + wh - h - m);
    // O cursor está sobre o ícone: descobre de que lado está a barra de tarefas.
    let (x, y) = if py >= wy + wh - 1.0 {
        (clamp_x(px - w / 2.0), wy + wh - h - m) // barra embaixo
    } else if py < wy + 1.0 {
        (clamp_x(px - w / 2.0), wy + m) // barra em cima
    } else if px < wx + 1.0 {
        (wx + m, clamp_y(py - h / 2.0)) // barra à esquerda
    } else {
        (wx + ww - w - m, clamp_y(py - h / 2.0)) // barra à direita ou sem âncora
    };
    window.set_position(slint::PhysicalPosition::new(x.round() as i32, y.round() as i32));

    let _ = component.show();
    window.with_winit_window(|w: &winit::window::Window| {
        use winit::platform::windows::WindowExtWindows;
        w.set_skip_taskbar(true);
        w.focus_window();
    });
    if let Some(hwnd) = hwnd(window) {
        winutil::round_corners(hwnd);
        winutil::bring_to_front(hwnd);
    }
}

/// Esconde o popup quando ele perde o foco (clique fora, Alt+Tab).
pub fn hide_on_blur(component: &impl ComponentHandle, on_hide: impl Fn() + 'static) {
    component.window().on_winit_window_event(move |_w, ev| {
        if let winit::event::WindowEvent::Focused(false) = ev {
            on_hide();
        }
        slint::winit_030::EventResult::Propagate
    });
}
