//! Liga a interface Slint ao núcleo: desenha o estado e transforma cliques em ações.
//! Tudo aqui roda na thread principal (a do laço de eventos).

slint::include_modules!();

use crate::core::{AuthMethod, Conn, Core, Snapshot};
use crate::device::{Control, Features};
use crate::i18n::{self, tr};
use crate::state::Action;
use crate::{icon, login, secrets, session, sync, window, winutil};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Abrir o popup só busca o status se o último tiver mais que isso.
const OPEN_REFRESH_AFTER: Duration = Duration::from_secs(30);
/// Espera antes de mandar a temperatura (vários cliques em +/− viram um envio só).
const TEMP_DEBOUNCE: Duration = Duration::from_millis(450);

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// Executa `f` com a interface, na thread principal.
fn with_ui(f: impl FnOnce(&Rc<Ui>)) {
    UI.with(|u| {
        if let Some(ui) = u.borrow().as_ref() {
            f(ui);
        }
    });
}

/// Agenda `f` na thread principal (de qualquer thread).
fn on_ui(f: impl FnOnce(&Rc<Ui>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || with_ui(f));
}

pub struct Ui {
    core: Arc<Core>,
    popup: Popup,
    tray: Tray,
    hidden_at: Cell<Option<Instant>>,
    sheet_kind: RefCell<String>,
    pending_temp: Cell<Option<f64>>,
    temp_timer: slint::Timer,
    flash_timer: slint::Timer,
    tick: slint::Timer,
    /// Último ícone desenhado (ligado, modo, temperatura), para não recriar à toa.
    tray_key: RefCell<Option<(bool, String, Option<i64>)>>,
}

/// Cria a interface, liga os eventos e começa a sincronizar.
pub fn start(core: Arc<Core>, show_event: isize) -> Result<(), slint::PlatformError> {
    let popup = Popup::new()?;
    let tray = Tray::new()?;
    popup.global::<Tr>().set_s(strings());
    tray.global::<Tr>().set_s(strings());
    popup.set_version(format!("{} {}", tr("Versão", "Version"), env!("CARGO_PKG_VERSION")).into());
    popup.set_oauth_available(core.oauth_app.is_some());

    let ui = Rc::new(Ui {
        core: core.clone(),
        popup,
        tray,
        hidden_at: Cell::new(None),
        sheet_kind: RefCell::new(String::new()),
        pending_temp: Cell::new(None),
        temp_timer: slint::Timer::default(),
        flash_timer: slint::Timer::default(),
        tick: slint::Timer::default(),
        tray_key: RefCell::new(None),
    });
    UI.with(|u| *u.borrow_mut() = Some(ui.clone()));
    wire(&ui);

    // O núcleo avisa mudanças de qualquer thread; junta vários avisos num desenho só.
    let pending = Arc::new(AtomicBool::new(false));
    core.on_change(move || {
        if !pending.swap(true, Ordering::AcqRel) {
            let pending = pending.clone();
            let _ = slint::invoke_from_event_loop(move || {
                pending.store(false, Ordering::Release);
                with_ui(|u| u.render());
            });
        }
    });

    // Outra cópia aberta pede para mostrar o popup.
    winutil::on_show_request(show_event, || on_ui(|u| u.show(None)));

    // "atualizado há X s" no rodapé.
    let weak = Rc::downgrade(&ui);
    ui.tick
        .start(slint::TimerMode::Repeated, Duration::from_secs(10), move || {
            if let Some(u) = weak.upgrade() {
                u.render_status(&u.core.snapshot());
            }
        });

    ui.tray.show()?;
    ui.render();

    // Desenvolvimento: ARCOND_OPEN=1 abre o popup ao iniciar (para testes e capturas de tela).
    if cfg!(debug_assertions) && std::env::var_os("ARCOND_OPEN").is_some() {
        slint::Timer::single_shot(Duration::from_millis(300), || with_ui(|u| u.show(None)));
    }

    // Polling em segundo plano.
    let c = core.clone();
    core.rt.spawn(async move {
        loop {
            sync::refresh(&c, false).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
    Ok(())
}

fn strings() -> Strings {
    let s = |pt: &'static str, en: &'static str| SharedString::from(tr(pt, en));
    Strings {
        app_name: s("Controle do Ar", "Air Remote"),
        airconditioner: s("Ar-condicionado", "Air conditioner"),
        off: s("Desligado", "Off"),
        offline: s("Aparelho offline", "Device offline"),
        mode: s("Modo", "Mode"),
        set_temp: s("Definir temperatura", "Set temperature"),
        no_temp_in_fan: s("Sem temperatura no modo Ventilar", "No temperature in Fan mode"),
        fan_speed: s("Intensidade do vento", "Fan speed"),
        air_direction: s("Direção do vento", "Air direction"),
        windfree: s("WindFree", "WindFree"),
        speed: s("Máx. (rápido)", "Max (fast)"),
        quiet: s("Silencioso", "Quiet"),
        light: s("Luz do display", "Display light"),
        light_toggle: s("Acender/apagar", "On/off"),
        night: s("Modo noite", "Night mode"),
        activate: s("Ativar", "Turn on"),
        cancel: s("Cancelar", "Cancel"),
        settings: s("Configurações", "Settings"),
        device: s("Aparelho", "Device"),
        account: s("Conta", "Account"),
        sign_in: s("Entrar com SmartThings", "Sign in with SmartThings"),
        pat_expires: s("O token pessoal expira em 24h", "Personal tokens expire after 24h"),
        sign_out: s("Sair da conta", "Sign out"),
        beep: s("Bipe", "Beep"),
        beep_mute: s("Silenciar", "Mute"),
        beep_on: s("Ativar", "Turn on"),
        auto_clean: s("Limpeza automática", "Auto clean"),
        filter: s("Filtro", "Filter"),
        autostart: s("Iniciar com o Windows", "Start with Windows"),
        about: s("Sobre", "About"),
        about_text: s(
            "Controle remoto de ar-condicionado Samsung pela API da SmartThings. Código aberto, licença MIT. Não é afiliado à Samsung.",
            "Remote for Samsung air conditioners through the SmartThings API. Open source, MIT license. Not affiliated with Samsung.",
        ),
        quit: s("Fechar o app", "Quit app"),
        login_title: s("Conectar ao SmartThings", "Connect to SmartThings"),
        login_text: s(
            "O login abre no seu navegador (funciona com conta Google). Depois de autorizar, o app conecta sozinho, e o login é renovado automaticamente.",
            "The login opens in your browser (Google sign-in works). After you authorize it, the app connects by itself, and the login is renewed automatically.",
        ),
        login_wait: s("Continue no navegador…", "Continue in your browser…"),
        manual_fold: s("O app não conectou sozinho?", "The app didn't connect by itself?"),
        manual_step1: s(
            "Abra o login alternativo no navegador e autorize.",
            "Open the alternative login in your browser and authorize it.",
        ),
        manual_step2: s(
            "A página final mostra um texto com code. Copie o endereço da página (ou o texto todo) e cole abaixo.",
            "The final page shows a text with code. Copy the page address (or the whole text) and paste it below.",
        ),
        open_browser: s("Abrir login alternativo", "Open alternative login"),
        paste_placeholder: s("Cole o endereço ou o código", "Paste the address or the code"),
        pat_fold: s("Usar token pessoal (PAT, vale 24h)", "Use a personal access token (PAT, valid 24h)"),
        token_placeholder: s("Token", "Token"),
        ok: s("OK", "OK"),
        power: s("Ligar/desligar", "Power"),
        temp_down: s("Diminuir temperatura", "Lower temperature"),
        temp_up: s("Aumentar temperatura", "Raise temperature"),
        back: s("Voltar", "Back"),
        more: s("Mais opções", "More options"),
        tray_open: s("Abrir controle", "Open remote"),
        tray_temp_up: s("Temperatura +", "Temperature +"),
        tray_temp_down: s("Temperatura −", "Temperature −"),
        tray_night: s("Modo noite", "Night mode"),
        tray_light: s("Acender/apagar luz do display", "Display light on/off"),
        tray_quit: s("Sair", "Quit"),
        mode_auto: i18n::mode_label("auto").into(),
        mode_cool: i18n::mode_label("cool").into(),
        mode_dry: i18n::mode_label("dry").into(),
        mode_wind: i18n::mode_label("wind").into(),
        mode_heat: i18n::mode_label("heat").into(),
    }
}

/// 24 → "24", 23.5 → "23.5".
fn fmt_temp(t: f64) -> String {
    if t.fract() == 0.0 {
        format!("{t:.0}")
    } else {
        format!("{t:.1}")
    }
}

fn ago(ms: u64) -> String {
    let s = crate::core::now_ms().saturating_sub(ms) / 1000;
    let pt = i18n::lang() == i18n::Lang::Pt;
    match s {
        0..=9 => tr("agora", "just now").into(),
        10..=59 if pt => format!("há {s} s"),
        10..=59 => format!("{s} s ago"),
        _ if pt => format!("há {} min", (s + 30) / 60),
        _ => format!("{} min ago", (s + 30) / 60),
    }
}

fn wire(ui: &Rc<Ui>) {
    let w = Rc::downgrade(ui);
    let p = &ui.popup;
    let t = &ui.tray;

    // Atalho: cria um callback que roda com a interface viva.
    fn cb<A>(w: &Weak<Ui>, f: impl Fn(&Rc<Ui>, A) + 'static) -> impl Fn(A) + 'static {
        let w = w.clone();
        move |a| {
            if let Some(u) = w.upgrade() {
                f(&u, a)
            }
        }
    }
    fn cb0(w: &Weak<Ui>, f: impl Fn(&Rc<Ui>) + 'static) -> impl Fn() + 'static {
        let w = w.clone();
        move || {
            if let Some(u) = w.upgrade() {
                f(&u)
            }
        }
    }

    // Popup
    p.on_power(cb0(&w, |u| {
        if let Some(s) = u.core.snapshot().state {
            u.act(Action::Power(!s.power), None);
        }
    }));
    p.on_temp_preview(cb(&w, |u, v: f32| {
        u.pending_temp.set(Some(v as f64));
        u.temp_timer.stop();
    }));
    p.on_temp_set(cb(&w, |u, v: f32| u.choose_temp(v as f64)));
    p.on_open_sheet(cb(&w, |u, kind: SharedString| u.open_sheet(&kind)));
    p.on_pick(cb(&w, |u, key: SharedString| u.pick(&key)));
    p.on_close_sheet(cb0(&w, |u| u.popup.set_sheet_open(false)));
    p.on_toggle_optional(cb(&w, |u, key: SharedString| {
        let current = u
            .core
            .snapshot()
            .state
            .and_then(|s| s.optional_mode)
            .unwrap_or_default();
        let on = if key == "windFree" {
            current.starts_with("windFree")
        } else {
            current == key.as_str()
        };
        u.act(
            Action::OptionalMode(if on { "off".into() } else { key.to_string() }),
            None,
        );
    }));
    p.on_light(cb0(&w, |u| u.act(Action::LightToggle, Some("light"))));
    p.on_light_set(cb(&w, |u, on: bool| u.act(Action::Light(on), None)));
    p.on_night(cb0(&w, |u| u.act(Action::NightMode, Some("night"))));
    p.on_beep(cb(&w, |u, on: bool| {
        u.act(Action::Beep(on), Some(if on { "beep-on" } else { "beep-off" }))
    }));
    p.on_autoclean(cb0(&w, |u| {
        if let Some(s) = u.core.snapshot().state {
            u.act(Action::AutoClean(!s.auto_clean), None);
        }
    }));
    p.on_autostart(cb0(&w, |u| {
        let _ = winutil::set_autostart(!winutil::autostart_enabled());
        u.popup.set_autostart_on(winutil::autostart_enabled());
    }));
    p.on_choose_device(cb0(&w, |u| u.open_device_sheet()));
    p.on_view_changed(cb(&w, |u, v: SharedString| u.view_changed(&v)));
    p.on_sign_out(cb0(&w, |u| u.sign_out()));
    p.on_login(cb0(&w, |u| u.login()));
    p.on_login_cancel(cb0(&w, |u| u.core.login_cancel.notify_waiters()));
    p.on_manual_open(cb0(&w, |u| {
        if let Err(e) = login::manual_start(&u.core) {
            u.popup.set_login_error(e.into());
        }
    }));
    p.on_manual_submit(cb(&w, |u, text: SharedString| u.manual_submit(text.to_string())));
    p.on_pat_submit(cb(&w, |u, text: SharedString| u.pat_submit(text.to_string())));
    p.on_quit(cb0(&w, |_| {
        let _ = slint::quit_event_loop();
    }));
    p.on_escape(cb0(&w, |u| u.escape()));
    {
        let w = w.clone();
        window::hide_on_blur(p, move || {
            if let Some(u) = w.upgrade() {
                u.hide();
            }
        });
    }

    // Bandeja
    t.on_open(cb0(&w, |u| u.toggle()));
    t.on_toggle_power(cb0(&w, |u| {
        if let Some(s) = u.core.snapshot().state {
            u.act(Action::Power(!s.power), None);
        }
    }));
    t.on_temp_step(cb(&w, |u, dir: i32| {
        let snap = u.core.snapshot();
        if let (Some(s), Some(f)) = (snap.state, snap.features) {
            let t = f.clamp_temp(s.setpoint + f.temp_step * dir as f64);
            if t != s.setpoint {
                u.act(Action::Setpoint(t), None);
            }
        }
    }));
    t.on_set_mode(cb(&w, |u, m: SharedString| u.act(Action::Mode(m.to_string()), None)));
    t.on_night(cb0(&w, |u| u.act(Action::NightMode, Some("night"))));
    t.on_light(cb0(&w, |u| u.act(Action::LightToggle, Some("light"))));
    t.on_quit(cb0(&w, |_| {
        let _ = slint::quit_event_loop();
    }));
}

impl Ui {
    // ---------- Janela ----------

    fn show(&self, anchor: Option<(i32, i32)>) {
        self.popup.set_sheet_open(false);
        if self.popup.get_view() == "settings" || self.popup.get_view() == "about" {
            self.popup.set_view("main".into());
        }
        window::show(&self.popup, anchor);
        let stale = self
            .core
            .inner
            .lock()
            .unwrap()
            .last_refresh
            .map(|t| t.elapsed() > OPEN_REFRESH_AFTER)
            .unwrap_or(true);
        if stale {
            let c = self.core.clone();
            self.core.rt.spawn(async move { sync::refresh(&c, false).await });
        }
    }

    fn hide(&self) {
        let _ = self.popup.hide();
        self.hidden_at.set(Some(Instant::now()));
    }

    fn toggle(&self) {
        if self.popup.window().is_visible() {
            self.hide();
            return;
        }
        // Clicar no ícone tira o foco do popup (que se esconde) antes do clique chegar aqui.
        let just_hidden = self
            .hidden_at
            .get()
            .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
        if !just_hidden {
            self.show(winutil::cursor_pos());
        }
    }

    fn escape(&self) {
        if self.popup.get_sheet_open() {
            self.popup.set_sheet_open(false);
        } else if self.popup.get_view() == "about" {
            self.popup.set_view("settings".into());
        } else if self.popup.get_view() == "settings" {
            self.popup.set_view("main".into());
        } else {
            self.hide();
        }
    }

    // ---------- Ações ----------

    /// Manda a ação em segundo plano. `flash` marca o botão (sem estado) com o resultado.
    fn act(&self, action: Action, flash: Option<&'static str>) {
        let c = self.core.clone();
        self.core.rt.spawn(async move {
            let ok = sync::act(&c, action).await.is_ok();
            if let Some(key) = flash {
                on_ui(move |u| u.flash(key, ok));
            }
        });
    }

    fn flash(&self, key: &str, ok: bool) {
        self.popup.set_flash_key(key.into());
        self.popup.set_flash_value(if ok { 1 } else { 2 });
        let weak = self.popup.as_weak();
        self.flash_timer
            .start(slint::TimerMode::SingleShot, Duration::from_millis(1400), move || {
                if let Some(p) = weak.upgrade() {
                    p.set_flash_value(0);
                }
            });
    }

    fn choose_temp(&self, t: f64) {
        let Some(f) = self.core.snapshot().features else { return };
        let t = f.clamp_temp(t);
        self.pending_temp.set(Some(t));
        self.popup.set_setpoint(t as f32);
        let weak = self.popup.as_weak();
        self.temp_timer
            .start(slint::TimerMode::SingleShot, TEMP_DEBOUNCE, move || {
                let _ = weak;
                with_ui(|u| {
                    let Some(value) = u.pending_temp.take() else { return };
                    if u.core.snapshot().state.is_some_and(|s| s.setpoint == value) {
                        return;
                    }
                    u.act(Action::Setpoint(value), None);
                });
            });
    }

    // ---------- Folha de opções ----------

    fn open_sheet(&self, kind: &str) {
        let snap = self.core.snapshot();
        let (Some(s), Some(f)) = (snap.state, snap.features) else {
            return;
        };
        let (title, options): (&str, Vec<SheetOption>) = match kind {
            "mode" => (
                tr("Modo", "Mode"),
                f.modes
                    .iter()
                    .map(|m| option(m, i18n::mode_label(m), &s.mode == m))
                    .collect(),
            ),
            "fan" => (
                tr("Intensidade do vento", "Fan speed"),
                f.fan_modes
                    .iter()
                    .map(|m| option(m, i18n::fan_label(m), s.fan.as_deref() == Some(m)))
                    .collect(),
            ),
            "osc" => (
                tr("Direção do vento", "Air direction"),
                f.oscillation_modes
                    .iter()
                    .map(|m| option(m, i18n::osc_label(m), s.oscillation.as_deref() == Some(m)))
                    .collect(),
            ),
            _ => return,
        };
        self.present_sheet(kind, title, options);
    }

    fn present_sheet(&self, kind: &str, title: &str, options: Vec<SheetOption>) {
        *self.sheet_kind.borrow_mut() = kind.to_string();
        self.popup.set_sheet_title(title.into());
        self.popup.set_sheet_options(ModelRc::new(VecModel::from(options)));
        self.popup.set_sheet_open(true);
    }

    fn pick(&self, key: &str) {
        self.popup.set_sheet_open(false);
        let selected = self
            .popup
            .get_sheet_options()
            .iter()
            .any(|o| o.key == key && o.selected);
        if selected {
            return;
        }
        let kind = self.sheet_kind.borrow().clone();
        match kind.as_str() {
            "mode" => self.act(Action::Mode(key.into()), None),
            "fan" => self.act(Action::Fan(key.into()), None),
            "osc" => self.act(Action::Oscillation(key.into()), None),
            "device" => self.select_device(key.to_string()),
            _ => {}
        }
    }

    fn open_device_sheet(&self) {
        let c = self.core.clone();
        self.core.rt.spawn(async move {
            let client = c.client.clone();
            let devices = session::with_token(&c, |t| {
                let cl = client.clone();
                async move { cl.devices(&t).await }
            })
            .await;
            let current = c.inner.lock().unwrap().cfg.device_id.clone();
            if let Ok(devices) = devices {
                let list: Vec<(String, String, bool)> = devices
                    .into_iter()
                    .filter(|d| d.is_ac)
                    .map(|d| (d.id.clone(), d.label, current.as_deref() == Some(&d.id)))
                    .collect();
                on_ui(move |u| {
                    let options = list
                        .iter()
                        .map(|(id, label, sel)| option(id, label.clone(), *sel))
                        .collect();
                    u.present_sheet("device", tr("Escolher aparelho", "Choose device"), options);
                });
            }
        });
    }

    fn select_device(&self, id: String) {
        {
            let mut i = self.core.inner.lock().unwrap();
            i.cfg.device_id = Some(id);
            let _ = i.cfg.save(&self.core.cfg_path);
            i.forget_device();
        }
        self.popup.set_view("main".into());
        let c = self.core.clone();
        self.core.rt.spawn(async move {
            sync::set_conn(&c, Conn::Loading, None);
            sync::refresh(&c, true).await;
        });
    }

    // ---------- Conta ----------

    fn view_changed(&self, view: &str) {
        self.popup.set_sheet_open(false);
        if view == "settings" {
            self.popup.set_autostart_on(winutil::autostart_enabled());
        }
        if view == "login" {
            self.popup.set_login_error("".into());
        }
        self.render();
    }

    fn login(&self) {
        self.popup.set_login_waiting(true);
        self.popup.set_login_error("".into());
        let c = self.core.clone();
        self.core.rt.spawn(async move {
            let result = match login::browser(&c).await {
                Ok(()) => session::finish_login(&c).await.map(|_| ()),
                Err(e) => Err(e),
            };
            on_ui(move |u| u.login_done(result));
        });
    }

    fn manual_submit(&self, text: String) {
        self.popup.set_login_error("".into());
        let c = self.core.clone();
        self.core.rt.spawn(async move {
            let result = match login::manual_finish(&c, &text).await {
                Ok(()) => session::finish_login(&c).await.map(|_| ()),
                Err(e) => Err(e),
            };
            on_ui(move |u| u.login_done(result));
        });
    }

    fn pat_submit(&self, token: String) {
        let token = token.trim().to_string();
        if token.is_empty() {
            return;
        }
        self.popup.set_login_error("".into());
        let c = self.core.clone();
        self.core.rt.spawn(async move {
            // Testa antes de trocar: um token errado não pode derrubar o login OAuth que já funciona.
            let result = async {
                c.client.devices(&token).await.map_err(|e| e.to_string())?;
                secrets::save(secrets::PAT, &token)?;
                c.inner.lock().unwrap().pat = Some(token);
                session::store_tokens(&c, None);
                session::finish_login(&c).await.map(|_| ())
            }
            .await;
            on_ui(move |u| u.login_done(result));
        });
    }

    fn login_done(&self, result: Result<(), String>) {
        self.popup.set_login_waiting(false);
        match result {
            Ok(()) => {
                self.popup.set_view("main".into());
                self.show(None);
            }
            Err(e) if e == session::cancelled() => {}
            Err(e) => self.popup.set_login_error(e.into()),
        }
        self.render();
    }

    fn sign_out(&self) {
        self.core.login_cancel.notify_waiters();
        session::store_tokens(&self.core, None);
        secrets::delete(secrets::PAT);
        {
            let mut i = self.core.inner.lock().unwrap();
            i.pat = None;
            i.forget_device();
        }
        sync::set_conn(&self.core, Conn::NoToken, None);
        self.popup.set_view("login".into());
        self.render();
    }

    // ---------- Desenho ----------

    fn render(&self) {
        let snap = self.core.snapshot();
        let p = &self.popup;

        if snap.conn == Conn::NoToken && p.get_view() != "login" {
            p.set_view("login".into());
            p.set_login_error(snap.message.clone().unwrap_or_default().into());
        }
        p.set_back_from_login(snap.conn != Conn::NoToken);
        p.set_device_label(snap.device_label.clone().unwrap_or_default().into());
        p.set_place(snap.place.clone().unwrap_or_default().into());
        p.set_account_text(
            match snap.auth {
                AuthMethod::OAuth => "SmartThings",
                AuthMethod::Pat => tr("Token pessoal", "Personal token"),
                AuthMethod::None => "--",
            }
            .into(),
        );
        p.set_show_sign_in(self.core.oauth_app.is_some() && snap.auth != AuthMethod::OAuth);
        p.set_show_sign_out(snap.auth != AuthMethod::None);
        self.render_status(&snap);
        self.render_tray(&snap);

        let (Some(s), Some(f)) = (snap.state.as_ref(), snap.features.as_ref()) else {
            p.set_loaded(false);
            p.set_filter_chip("".into());
            return;
        };
        p.set_loaded(snap.conn != Conn::NoToken);
        let on = s.power && s.online;
        p.set_on(on);
        p.set_mode(s.mode.clone().into());

        // Banner
        let (title, temp, humidity) = if !s.online {
            (
                tr("Aparelho offline", "Device offline").to_string(),
                String::new(),
                String::new(),
            )
        } else if s.power {
            let t = s.room_temp.unwrap_or(s.setpoint);
            let h = s.humidity.map(|h| format!("{}%", h.round())).unwrap_or_default();
            (i18n::state_title(&s.mode), format!("{}°{}", fmt_temp(t), f.unit), h)
        } else {
            (tr("Desligado", "Off").to_string(), String::new(), String::new())
        };
        p.set_hero_title(title.into());
        p.set_hero_temp(temp.into());
        p.set_hero_humidity(humidity.into());

        // Modo e temperatura
        p.set_show_mode(!f.modes.is_empty());
        p.set_mode_label(i18n::mode_label(&s.mode).into());
        p.set_unit(f.unit.clone().into());
        p.set_temp_min(f.temp_min as f32);
        p.set_temp_max(f.temp_max as f32);
        p.set_temp_step(f.temp_step as f32);
        p.set_setpoint(self.pending_temp.get().unwrap_or(s.setpoint) as f32);

        // Vento
        p.set_show_fan(!f.fan_modes.is_empty());
        p.set_fan_label(
            s.fan
                .as_deref()
                .map(i18n::fan_label)
                .unwrap_or_else(|| "--".into())
                .into(),
        );
        p.set_show_osc(!f.oscillation_modes.is_empty());
        p.set_osc_label(
            s.oscillation
                .as_deref()
                .map(i18n::osc_label)
                .unwrap_or_else(|| "--".into())
                .into(),
        );

        // Opções
        let optional = s.optional_mode.clone().unwrap_or_else(|| "off".into());
        p.set_show_windfree(f.has_optional("windFree"));
        p.set_windfree_on(optional.starts_with("windFree"));
        p.set_show_speed(f.has_optional("speed"));
        p.set_speed_on(optional == "speed");
        p.set_show_quiet(f.has_optional("quiet"));
        p.set_quiet_on(optional == "quiet");
        p.set_light_kind(match (f.light, s.light) {
            (Control::None, _) => 0,
            (Control::Direct, Some(_)) => 2,
            _ => 1,
        });
        p.set_light_on(s.light.unwrap_or(false));
        p.set_show_night(f.night_mode());
        p.set_night_hint(night_hint(f).into());

        // Configurações do ar
        p.set_show_beep(f.beep != Control::None);
        p.set_show_autoclean(f.auto_clean);
        p.set_autoclean_on(s.auto_clean);
        p.set_show_filter(f.filter);
        let fs = s.filter_status.clone();
        p.set_filter_text(
            fs.as_deref()
                .map(i18n::filter_label)
                .unwrap_or_else(|| "--".into())
                .into(),
        );
        let dirty = fs.as_deref().is_some_and(|v| v != "normal");
        p.set_filter_warn(dirty);
        p.set_filter_chip(
            if f.filter && dirty && snap.conn != Conn::NoToken {
                if fs.as_deref() == Some("replace") {
                    tr("Trocar filtro", "Replace filter")
                } else {
                    tr("Lavar filtro", "Wash filter")
                }
            } else {
                ""
            }
            .into(),
        );
    }

    fn render_status(&self, snap: &Snapshot) {
        let offline = snap.state.as_ref().is_some_and(|s| !s.online);
        let (kind, text): (&str, String) = match snap.conn {
            Conn::Ok if offline => ("warn", tr("Aparelho offline", "Device offline").into()),
            Conn::Ok => (
                "ok",
                match snap.updated_at {
                    Some(t) => format!(
                        "{} · {} {}",
                        tr("Conectado", "Connected"),
                        tr("atualizado", "updated"),
                        ago(t)
                    ),
                    None => tr("Conectado", "Connected").into(),
                },
            ),
            Conn::Sending => ("sending", tr("Enviando…", "Sending…").into()),
            Conn::Offline => (
                "warn",
                tr("Sem conexão com a internet", "No internet connection").into(),
            ),
            Conn::NoToken => ("warn", tr("Faça login para continuar", "Sign in to continue").into()),
            Conn::RateLimited => (
                "warn",
                snap.message
                    .clone()
                    .unwrap_or_else(|| tr("Limite da API atingido", "API rate limit reached").into()),
            ),
            Conn::Error => (
                "err",
                snap.message.clone().unwrap_or_else(|| tr("Erro", "Error").into()),
            ),
            Conn::Loading => ("loading", tr("Carregando…", "Loading…").into()),
        };
        self.popup.set_status_kind(kind.into());
        self.popup.set_status_text(text.into());
    }

    fn render_tray(&self, snap: &Snapshot) {
        let st = snap.state.as_ref();
        let usable = st.is_some() && matches!(snap.conn, Conn::Ok | Conn::Sending | Conn::Loading);
        let key = (
            st.is_some_and(|s| s.power && s.online),
            st.map(|s| s.mode.clone()).unwrap_or_else(|| "cool".into()),
            if usable {
                st.map(|s| s.setpoint.round() as i64)
            } else {
                None
            },
        );
        if self.tray_key.borrow().as_ref() != Some(&key) {
            let (rgba, size) = icon::render(key.0, &key.1, key.2);
            let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, size, size);
            self.tray.set_tray_icon(slint::Image::from_rgba8(buf));
            *self.tray_key.borrow_mut() = Some(key);
        }

        let unit = snap.features.as_ref().map(|f| f.unit.as_str()).unwrap_or("C");
        let app = tr("Controle do Ar", "Air Remote");
        let tooltip = match (st, snap.conn) {
            (_, Conn::NoToken) => format!("{app}: {}", tr("faça login", "sign in")),
            (Some(s), _) if !s.online => format!("{app}: {}", tr("aparelho offline", "device offline")),
            (Some(s), _) => {
                let room = s
                    .room_temp
                    .map(|t| format!(" · {} {}°{unit}", tr("Quarto", "Room"), fmt_temp(t.round())))
                    .unwrap_or_default();
                if s.power {
                    format!(
                        "{} · {} {}°{unit}{room}",
                        tr("Ligado", "On"),
                        i18n::mode_label(&s.mode),
                        fmt_temp(s.setpoint)
                    )
                } else {
                    format!("{}{room}", tr("Desligado", "Off"))
                }
            }
            (None, _) => app.to_string(),
        };
        let t = &self.tray;
        t.set_tray_tooltip(tooltip.into());
        t.set_loaded(st.is_some() && snap.features.is_some() && snap.conn != Conn::NoToken);
        if let Some(s) = st {
            t.set_power_text(
                if s.power {
                    tr("Desligar", "Turn off")
                } else {
                    tr("Ligar", "Turn on")
                }
                .into(),
            );
            t.set_temp_text(format!("{}: {}°{unit}", tr("Temperatura", "Temperature"), fmt_temp(s.setpoint)).into());
            t.set_mode(s.mode.clone().into());
        } else {
            t.set_power_text(tr("Ligar", "Turn on").into());
            t.set_temp_text(format!("{}: --", tr("Temperatura", "Temperature")).into());
        }
        if let Some(f) = &snap.features {
            let modes: Vec<SharedString> = f.modes.iter().map(|m| m.as_str().into()).collect();
            t.set_modes(ModelRc::new(VecModel::from(modes)));
            t.set_night_enabled(f.night_mode());
            t.set_light_enabled(f.light != Control::None);
        }
    }
}

fn option(key: &str, label: String, selected: bool) -> SheetOption {
    SheetOption {
        key: key.into(),
        label: label.into(),
        selected,
    }
}

fn night_hint(f: &Features) -> String {
    let mut parts = Vec::new();
    if f.light != Control::None {
        parts.push(tr("luz apagada", "light off"));
    }
    if f.beep != Control::None {
        parts.push(tr("bipe mudo", "beep muted"));
    }
    if f.has_optional("windFree") {
        parts.push("WindFree");
    }
    i18n::list_text(&parts)
}
