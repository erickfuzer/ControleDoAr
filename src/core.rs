//! Estado central do app, compartilhado entre a interface (thread principal) e as tarefas de rede.
//!
//! A interface nunca lê a rede direto: ela pede ações (`sync::act`) e é avisada por `publish`
//! quando o estado muda. Nenhum lock é segurado enquanto se mexe na interface.

use crate::api::Client;
use crate::auth::{OAuthApp, Tokens};
use crate::config::{self, Config};
use crate::device::Features;
use crate::secrets;
use crate::state::AcState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conn {
    Loading,
    Ok,
    Sending,
    Offline,
    NoToken,
    RateLimited,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    None,
    OAuth,
    Pat,
}

#[derive(Default)]
pub struct Inner {
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

pub struct Core {
    pub client: Client,
    pub oauth_app: Option<OAuthApp>,
    /// Um refresh por vez: o refresh token é de uso único.
    pub refresh_lock: tokio::sync::Mutex<()>,
    pub login_cancel: tokio::sync::Notify,
    pub manual_state: Mutex<Option<String>>,
    pub cfg_path: PathBuf,
    pub inner: Mutex<Inner>,
    pub rt: tokio::runtime::Handle,
    on_change: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

/// Cópia do estado para a interface desenhar.
#[derive(Clone)]
pub struct Snapshot {
    pub state: Option<AcState>,
    pub features: Option<Features>,
    pub conn: Conn,
    pub message: Option<String>,
    pub updated_at: Option<u64>,
    pub device_label: Option<String>,
    pub place: Option<String>,
    pub auth: AuthMethod,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Core {
    pub fn new(rt: tokio::runtime::Handle) -> Arc<Self> {
        let cfg_path = config::default_path();
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
        Arc::new(Self {
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
            rt,
            on_change: OnceLock::new(),
        })
    }

    /// A interface registra aqui como quer ser avisada de mudanças.
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.on_change.set(Box::new(f));
    }

    pub fn publish(&self) {
        if let Some(f) = self.on_change.get() {
            f();
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let i = self.inner.lock().unwrap();
        Snapshot {
            state: i.state.clone(),
            features: i.features.clone(),
            conn: i.conn.unwrap_or(Conn::Loading),
            message: i.message.clone(),
            updated_at: i.updated_at,
            device_label: i.device_label.clone(),
            place: i.place.clone(),
            auth: if i.oauth.is_some() {
                AuthMethod::OAuth
            } else if i.pat.is_some() {
                AuthMethod::Pat
            } else {
                AuthMethod::None
            },
        }
    }
}
