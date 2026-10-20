//! Comandos que o popup pode chamar (`invoke`). Só a janela principal, com conteúdo
//! local, tem acesso: páginas remotas (como a de login) são bloqueadas pelo Tauri.

use crate::api::DeviceInfo;
use crate::i18n::tr;
use crate::session::{self, with_token};
use crate::state::Action;
use crate::{auth, popup, secrets, shared, snapshot, sync, Conn, Snapshot};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn get_snapshot(app: AppHandle) -> Snapshot {
    snapshot(&app)
}

#[tauri::command]
pub async fn refresh(app: AppHandle) -> Snapshot {
    sync::refresh(&app, true).await;
    snapshot(&app)
}

#[tauri::command]
pub async fn act(app: AppHandle, action: Action) -> Result<(), String> {
    sync::act(&app, action).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceChoice {
    id: String,
    label: String,
    selected: bool,
}

/// Ar-condicionados da conta, para escolher qual controlar.
#[tauri::command]
pub async fn list_devices(app: AppHandle) -> Result<Vec<DeviceChoice>, String> {
    let s = shared(&app);
    let client = s.client.clone();
    let devices: Vec<DeviceInfo> = with_token(&app, |t| {
        let c = client.clone();
        async move { c.devices(&t).await }
    })
    .await
    .map_err(|e| e.to_string())?;
    let current = s.inner.lock().unwrap().cfg.device_id.clone();
    Ok(devices
        .into_iter()
        .filter(|d| d.is_ac)
        .map(|d| DeviceChoice {
            selected: current.as_deref() == Some(&d.id),
            id: d.id,
            label: d.label,
        })
        .collect())
}

#[tauri::command]
pub async fn select_device(app: AppHandle, id: String) -> Result<Snapshot, String> {
    {
        let s = shared(&app);
        let mut i = s.inner.lock().unwrap();
        i.cfg.device_id = Some(id);
        i.cfg.save(&s.cfg_path).map_err(|e| e.to_string())?;
        i.forget_device();
    }
    sync::set_conn(&app, Conn::Loading, None);
    sync::refresh(&app, true).await;
    Ok(snapshot(&app))
}

#[tauri::command]
pub async fn set_token(app: AppHandle, token: String) -> Result<Snapshot, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(tr("Token vazio", "Empty token").into());
    }
    // Testa antes de trocar: um token errado não pode derrubar o login OAuth que já funciona.
    let client = shared(&app).client.clone();
    client.devices(&token).await.map_err(|e| e.to_string())?;
    secrets::save(secrets::PAT, &token)?;
    shared(&app).inner.lock().unwrap().pat = Some(token);
    // O PAT escolhido manualmente substitui o login OAuth.
    session::store_tokens(&app, None);
    session::finish_login(&app).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfo {
    oauth_available: bool,
    method: &'static str,
}

#[tauri::command]
pub fn auth_info(app: AppHandle) -> AuthInfo {
    let s = shared(&app);
    let i = s.inner.lock().unwrap();
    AuthInfo {
        oauth_available: s.oauth_app.is_some(),
        method: if i.oauth.is_some() {
            "oauth"
        } else if i.pat.is_some() {
            "pat"
        } else {
            "none"
        },
    }
}

/// Login OAuth numa janela do próprio app.
#[tauri::command]
pub async fn login_oauth(app: AppHandle) -> Result<Snapshot, String> {
    let oa = shared(&app).oauth_app.clone().ok_or_else(session::not_configured)?;
    shared(&app).login_cancel.notify_waiters();
    let result = async {
        let code = session::login_window_code(&app, &oa).await?;
        session::exchange_code(&app, &code).await?;
        session::finish_login(&app).await
    }
    .await;
    // O popup se escondeu quando a janela de login ganhou o foco; reabre com o resultado.
    popup::show(&app, None);
    result
}

/// Plano B (ex.: conta Google, que não deixa logar em janela embutida): login no navegador
/// do sistema; o usuário cola o endereço final.
#[tauri::command]
pub fn login_manual_start(app: AppHandle) -> Result<(), String> {
    let s = shared(&app);
    let oa = s.oauth_app.clone().ok_or_else(session::not_configured)?;
    s.login_cancel.notify_waiters();
    let state = auth::random_state();
    *s.manual_state.lock().unwrap() = Some(state.clone());
    app.opener()
        .open_url(auth::authorize_url(&oa, auth::REDIRECT_URI, &state), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn login_manual_finish(app: AppHandle, input: String) -> Result<Snapshot, String> {
    let code = auth::extract_code(&input).ok_or_else(|| {
        tr(
            "Não encontrei o código no texto colado",
            "No code found in the pasted text",
        )
        .to_string()
    })?;
    let expected = shared(&app).manual_state.lock().unwrap().clone();
    if let (Some(exp), Some(i)) = (expected, input.find("state")) {
        if !input[i..].contains(&exp) {
            return Err(tr(
                "Esse endereço é de outra tentativa de login",
                "That address is from another login attempt",
            )
            .into());
        }
    }
    session::exchange_code(&app, &code).await?;
    session::finish_login(&app).await
}

#[tauri::command]
pub fn cancel_login(app: AppHandle) {
    shared(&app).login_cancel.notify_waiters();
}

#[tauri::command]
pub fn logout(app: AppHandle) -> Snapshot {
    shared(&app).login_cancel.notify_waiters();
    session::store_tokens(&app, None);
    secrets::delete(secrets::PAT);
    {
        let s = shared(&app);
        let mut i = s.inner.lock().unwrap();
        i.pat = None;
        i.forget_device();
    }
    sync::set_conn(&app, Conn::NoToken, None);
    snapshot(&app)
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    popup::hide(&app);
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|e| e.to_string())?;
    Ok(al.is_enabled().unwrap_or(false))
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
