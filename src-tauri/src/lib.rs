//! Controle do Ar: controle remoto de ar-condicionado Samsung (SmartThings) na bandeja do Windows.
//!
//! Módulos:
//! - `api`, `auth`: clientes HTTP da SmartThings (dispositivos e OAuth)
//! - `device`: o que cada modelo suporta, lido do status
//! - `state`: estado do ar e as ações que viram comandos
//! - `sync`: busca de status, envio de comandos, UI otimista e limite da API
//! - `session`: credenciais (OAuth com renovação ou PAT)
//! - `tray`, `popup`, `icon`: bandeja, janela e ícone
//! - `commands`: o que o frontend pode chamar

mod api;
mod auth;
mod commands;
mod config;
mod device;
mod i18n;
mod icon;
mod popup;
mod secrets;
mod session;
mod state;
mod sync;
mod tray;

use api::Client;
use auth::{OAuthApp, Tokens};
use config::Config;
use device::Features;
use serde::Serialize;
use state::AcState;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

const POLL_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Conn {
    Loading,
    Ok,
    Sending,
    Offline,
    NoToken,
    RateLimited,
    Error,
}

#[derive(Default)]
pub(crate) struct Inner {
    pub cfg: Config,
    /// Tokens OAuth (têm prioridade sobre o PAT).
    pub oauth: Option<Tokens>,
    /// PAT (Gerenciador de Credenciais; em desenvolvimento também do .env).
    pub pat: Option<String>,
    pub state: Option<AcState>,
    pub features: Option<Features>,
    pub device_label: Option<String>,
    /// "Casa - Quarto 1"
    pub place: Option<String>,
    pub conn: Option<Conn>,
    pub message: Option<String>,
    pub updated_at: Option<u64>,
    pub last_command: Option<Instant>,
    pub last_refresh: Option<Instant>,
    pub hidden_at: Option<Instant>,
    /// Depois de um 429, nada é enviado até este instante.
    pub blocked_until: Option<Instant>,
    pub health: Option<(Instant, bool)>,
    /// Incrementa a cada comando; só o refresh do último comando roda.
    pub cmd_gen: u64,
    /// Sem leitura da luz (modelos via `execute`), o app lembra o último envio.
    /// O display acende sempre que o ar liga, então volta a `false` nesse momento.
    pub display_dark: bool,
}

impl Inner {
    pub fn blocked_for(&self) -> Option<u64> {
        let left = self.blocked_until?.checked_duration_since(Instant::now())?;
        Some(left.as_secs() + 1)
    }

    /// Esquece o aparelho atual (troca de aparelho ou de conta).
    pub fn forget_device(&mut self) {
        self.state = None;
        self.features = None;
        self.device_label = None;
        self.place = None;
        self.health = None;
        self.display_dark = false;
    }
}

pub(crate) struct Shared {
    pub client: Client,
    pub oauth_app: Option<OAuthApp>,
    /// Um refresh por vez: o refresh token é de uso único.
    pub refresh_lock: tokio::sync::Mutex<()>,
    pub login_cancel: tokio::sync::Notify,
    pub manual_state: Mutex<Option<String>>,
    pub cfg_path: PathBuf,
    pub inner: Mutex<Inner>,
    /// Preenchido uma vez em `tray::build`. Sem Mutex de propósito: mexer no menu espera a
    /// thread principal, e segurar um lock nessa espera pode travar o app.
    pub menu: OnceLock<tray::TrayMenu>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    state: Option<AcState>,
    features: Option<Features>,
    conn: Conn,
    message: Option<String>,
    updated_at: Option<u64>,
    device_label: Option<String>,
    place: Option<String>,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn shared(app: &AppHandle) -> tauri::State<'_, Shared> {
    app.state::<Shared>()
}

pub(crate) fn snapshot(app: &AppHandle) -> Snapshot {
    let s = shared(app);
    let i = s.inner.lock().unwrap();
    Snapshot {
        state: i.state.clone(),
        features: i.features.clone(),
        conn: i.conn.unwrap_or(Conn::Loading),
        message: i.message.clone(),
        updated_at: i.updated_at,
        device_label: i.device_label.clone(),
        place: i.place.clone(),
    }
}

/// Envia o estado para o popup e atualiza ícone, dica e menu da bandeja.
pub(crate) fn publish(app: &AppHandle) {
    let snap = snapshot(app);
    let _ = app.emit("ac-update", &snap);
    tray::update(app, &snap);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            popup::show(app, None);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let cfg_path = app.path().app_config_dir()?.join("config.json");
            let mut cfg = Config::load(&cfg_path);
            // Migra o PAT que versões antigas deixavam em texto puro no config.json.
            if let Some(old) = cfg.legacy_token.take() {
                if secrets::load(secrets::PAT).is_none() {
                    let _ = secrets::save(secrets::PAT, &old);
                }
                let _ = cfg.save(&cfg_path);
            }
            let pat = secrets::load(secrets::PAT).or_else(config::dev_token);
            let oauth = secrets::load(secrets::OAUTH).and_then(|s| serde_json::from_str::<Tokens>(&s).ok());
            app.manage(Shared {
                client: Client::new(),
                oauth_app: OAuthApp::builtin(),
                refresh_lock: tokio::sync::Mutex::new(()),
                login_cancel: tokio::sync::Notify::new(),
                manual_state: Mutex::new(None),
                cfg_path,
                inner: Mutex::new(Inner {
                    cfg,
                    oauth,
                    pat,
                    ..Default::default()
                }),
                menu: OnceLock::new(),
            });

            let handle = app.handle().clone();
            if let Some(win) = popup::window(&handle) {
                // O popup não guarda nada no navegador; apaga cookies de logins antigos
                // que tenham ficado no perfil compartilhado do WebView2.
                let _ = win.clear_all_browsing_data();
                let h = handle.clone();
                win.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        popup::hide(&h);
                    }
                });
            }

            tray::build(&handle)?;
            publish(&handle);

            // Polling em segundo plano.
            tauri::async_runtime::spawn(async move {
                loop {
                    sync::refresh(&handle, false).await;
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::refresh,
            commands::act,
            commands::list_devices,
            commands::select_device,
            commands::set_token,
            commands::auth_info,
            commands::login_oauth,
            commands::login_manual_start,
            commands::login_manual_finish,
            commands::cancel_login,
            commands::logout,
            commands::hide_popup,
            commands::get_autostart,
            commands::set_autostart,
            commands::quit
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the app");
}
