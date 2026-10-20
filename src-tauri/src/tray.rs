//! Ícone e menu da bandeja.

use crate::device::Control;
use crate::i18n::{mode_label, tr};
use crate::state::Action;
use crate::{icon, popup, shared, sync, Conn, Snapshot};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

/// Todos os modos conhecidos; os que o aparelho não suporta ficam desativados.
const MODES: [&str; 5] = ["auto", "cool", "dry", "wind", "heat"];

pub struct TrayMenu {
    power: MenuItem<Wry>,
    temp: MenuItem<Wry>,
    up: MenuItem<Wry>,
    down: MenuItem<Wry>,
    night: MenuItem<Wry>,
    light: MenuItem<Wry>,
    modes: Vec<(&'static str, CheckMenuItem<Wry>)>,
}

/// 24 → "24", 23.5 → "23.5".
pub fn fmt_temp(t: f64) -> String {
    if t.fract() == 0.0 {
        format!("{t:.0}")
    } else {
        format!("{t:.1}")
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, text: &str, enabled: bool| MenuItem::with_id(app, id, text, enabled, None::<&str>);
    let power = item("power", tr("Ligar", "Turn on"), true)?;
    let temp = item("temp", tr("Temperatura: --", "Temperature: --"), false)?;
    let up = item("temp_up", tr("Temperatura +", "Temperature +"), true)?;
    let down = item("temp_down", tr("Temperatura −", "Temperature −"), true)?;
    let modes: Vec<(&'static str, CheckMenuItem<Wry>)> = MODES
        .iter()
        .map(|m| {
            CheckMenuItem::with_id(app, format!("mode:{m}"), mode_label(m), true, false, None::<&str>).map(|i| (*m, i))
        })
        .collect::<Result<_, _>>()?;
    let mode_refs: Vec<&dyn IsMenuItem<Wry>> = modes.iter().map(|(_, i)| i as &dyn IsMenuItem<Wry>).collect();
    let mode_menu = Submenu::with_items(app, tr("Modo", "Mode"), true, &mode_refs)?;
    let night = item("night", tr("Modo noite", "Night mode"), true)?;
    let light = item(
        "light",
        tr("Acender/apagar luz do display", "Display light on/off"),
        true,
    )?;
    let open = item("open", tr("Abrir controle", "Open remote"), true)?;
    let quit = item("quit", tr("Sair", "Quit"), true)?;
    let sep = || PredefinedMenuItem::separator(app);

    let menu = Menu::with_items(
        app,
        &[
            &power,
            &sep()?,
            &temp,
            &up,
            &down,
            &mode_menu,
            &sep()?,
            &night,
            &light,
            &sep()?,
            &open,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .icon({
            let (rgba, size) = icon::render(false, "cool", None);
            Image::new_owned(rgba, size, size)
        })
        .tooltip(tr("Controle do Ar", "Air Remote"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                popup::toggle(tray.app_handle(), position);
            }
        })
        .on_menu_event(on_menu)
        .build(app)?;

    let _ = shared(app).menu.set(TrayMenu {
        power,
        temp,
        up,
        down,
        night,
        light,
        modes,
    });
    Ok(())
}

fn on_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let (state, features) = {
        let s = shared(app);
        let i = s.inner.lock().unwrap();
        (i.state.clone(), i.features.clone())
    };
    let id = event.id().as_ref();
    match (id, state, features) {
        ("open", ..) => popup::show(app, None),
        ("quit", ..) => app.exit(0),
        ("power", Some(s), _) => sync::spawn_act(app, Action::Power(!s.power)),
        ("temp_up" | "temp_down", Some(s), Some(f)) => {
            let step = if id == "temp_up" { f.temp_step } else { -f.temp_step };
            let t = f.clamp_temp(s.setpoint + step);
            if t != s.setpoint {
                sync::spawn_act(app, Action::Setpoint(t));
            }
        }
        ("night", ..) => sync::spawn_act(app, Action::NightMode),
        ("light", ..) => sync::spawn_act(app, Action::LightToggle),
        (m, ..) if m.starts_with("mode:") => sync::spawn_act(app, Action::Mode(m[5..].to_string())),
        _ => {}
    }
    // O menu já marcou o item clicado; republica para refletir o estado real.
    crate::publish(app);
}

/// Atualiza ícone, dica e menu a partir do estado atual.
pub fn update(app: &AppHandle, snap: &Snapshot) {
    let st = snap.state.as_ref();
    let usable = st.is_some() && matches!(snap.conn, Conn::Ok | Conn::Sending | Conn::Loading);
    let (rgba, size) = icon::render(
        st.map(|s| s.power && s.online).unwrap_or(false),
        st.map(|s| s.mode.as_str()).unwrap_or("cool"),
        if usable {
            st.map(|s| s.setpoint.round() as i64)
        } else {
            None
        },
    );
    let unit = snap.features.as_ref().map(|f| f.unit.as_str()).unwrap_or("C");
    let app_name = tr("Controle do Ar", "Air Remote");
    let tooltip = match (st, snap.conn) {
        (_, Conn::NoToken) => format!("{app_name}: {}", tr("faça login", "sign in")),
        (Some(s), _) if !s.online => format!("{app_name}: {}", tr("aparelho offline", "device offline")),
        (Some(s), _) => {
            let room = s
                .room_temp
                .map(|t| format!(" · {} {}°{unit}", tr("Quarto", "Room"), fmt_temp(t.round())))
                .unwrap_or_default();
            if s.power {
                format!(
                    "{} · {} {}°{unit}{room}",
                    tr("Ligado", "On"),
                    mode_label(&s.mode),
                    fmt_temp(s.setpoint)
                )
            } else {
                format!("{}{room}", tr("Desligado", "Off"))
            }
        }
        (None, _) => app_name.to_string(),
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(Image::new_owned(rgba, size, size)));
        let _ = tray.set_tooltip(Some(&tooltip));
    }

    let s = shared(app);
    let Some(menu) = s.menu.get() else { return };
    if let Some(s) = st {
        let _ = menu.power.set_text(if s.power {
            tr("Desligar", "Turn off")
        } else {
            tr("Ligar", "Turn on")
        });
        let _ = menu.temp.set_text(format!(
            "{}: {}°{unit}",
            tr("Temperatura", "Temperature"),
            fmt_temp(s.setpoint)
        ));
        for (m, item) in &menu.modes {
            let _ = item.set_checked(*m == s.mode);
        }
    }
    if let Some(f) = &snap.features {
        for (m, item) in &menu.modes {
            let _ = item.set_enabled(f.modes.iter().any(|x| x == m));
        }
        let _ = menu.night.set_enabled(f.night_mode());
        let _ = menu.light.set_enabled(f.light != Control::None);
    }
    let loaded = st.is_some() && snap.features.is_some();
    for item in [&menu.power, &menu.up, &menu.down] {
        let _ = item.set_enabled(loaded);
    }
}
