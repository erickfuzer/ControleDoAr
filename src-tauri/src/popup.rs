//! A janela do controle: aparece colada na bandeja e some ao perder o foco.

use crate::{shared, sync};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Altura do popup (a mesma de `tauri.conf.json`); diminui em telas baixas, e o conteúdo rola.
const HEIGHT: f64 = 690.0;
/// Abrir o popup só busca o status se o último tiver mais que isso.
const OPEN_REFRESH_AFTER: Duration = Duration::from_secs(30);

pub fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Posiciona o popup colado na bandeja, respeitando a posição da barra de tarefas.
fn place(win: &WebviewWindow, anchor: Option<PhysicalPosition<f64>>) {
    let monitor = match anchor {
        Some(p) => win.monitor_from_point(p.x, p.y).ok().flatten(),
        None => None,
    }
    .or_else(|| win.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, win.inner_size()) else {
        return;
    };
    let wa = monitor.work_area();
    let (wx, wy) = (wa.position.x as f64, wa.position.y as f64);
    let (ww, wh) = (wa.size.width as f64, wa.size.height as f64);
    let m = 12.0 * monitor.scale_factor();
    let w = size.width as f64;
    let h = (HEIGHT * monitor.scale_factor()).min(wh - 2.0 * m).max(200.0).round();
    if (size.height as f64 - h).abs() >= 1.0 {
        let _ = win.set_size(PhysicalSize::new(size.width, h as u32));
    }
    let p = anchor.unwrap_or(PhysicalPosition::new(wx + ww, wy + wh + 1.0));

    // f64::clamp dá panic se min > max (tela menor que o popup); aqui o mínimo vence.
    let clamp = |v: f64, min: f64, max: f64| v.min(max).max(min);
    let clamp_x = |x: f64| clamp(x, wx + m, wx + ww - w - m);
    let clamp_y = |y: f64| clamp(y, wy + m, wy + wh - h - m);
    let (x, y) = if p.y >= wy + wh {
        (clamp_x(p.x - w / 2.0), wy + wh - h - m) // barra embaixo
    } else if p.y < wy {
        (clamp_x(p.x - w / 2.0), wy + m) // barra em cima
    } else if p.x < wx {
        (wx + m, clamp_y(p.y - h / 2.0)) // barra à esquerda
    } else {
        (wx + ww - w - m, clamp_y(p.y - h / 2.0)) // barra à direita / sem âncora
    };
    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

pub fn show(app: &AppHandle, anchor: Option<PhysicalPosition<f64>>) {
    let Some(win) = window(app) else { return };
    place(&win, anchor);
    let _ = win.show();
    let _ = win.set_focus();
    let _ = app.emit("popup-shown", ());

    let stale = shared(app)
        .inner
        .lock()
        .unwrap()
        .last_refresh
        .map(|t| t.elapsed() > OPEN_REFRESH_AFTER)
        .unwrap_or(true);
    if stale {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { sync::refresh(&app, false).await });
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(win) = window(app) {
        let _ = win.hide();
    }
    shared(app).inner.lock().unwrap().hidden_at = Some(Instant::now());
}

pub fn toggle(app: &AppHandle, anchor: PhysicalPosition<f64>) {
    let Some(win) = window(app) else { return };
    if win.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }
    // Clicar no ícone tira o foco do popup (que se esconde) antes do clique chegar aqui.
    let just_hidden = shared(app)
        .inner
        .lock()
        .unwrap()
        .hidden_at
        .map(|t| t.elapsed() < Duration::from_millis(250))
        .unwrap_or(false);
    if !just_hidden {
        show(app, Some(anchor));
    }
}
