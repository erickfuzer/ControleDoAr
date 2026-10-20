//! Credenciais: tokens OAuth (com renovação automática) ou PAT, e a janela de login.

use crate::api::ApiError;
use crate::auth::{self, AuthError, OAuthApp, Tokens};
use crate::i18n::tr;
use crate::{secrets, shared, snapshot, sync, Conn, Snapshot};
use std::future::Future;
use std::time::Duration;
use tauri::{AppHandle, Manager, WindowEvent};

pub fn store_tokens(app: &AppHandle, tokens: Option<Tokens>) {
    match &tokens {
        Some(t) => {
            if let Err(e) = secrets::save(secrets::OAUTH, &serde_json::to_string(t).unwrap_or_default()) {
                eprintln!("could not save tokens to Credential Manager: {e}");
            }
        }
        None => secrets::delete(secrets::OAUTH),
    }
    shared(app).inner.lock().unwrap().oauth = tokens;
}

pub fn has_credentials(app: &AppHandle) -> bool {
    let s = shared(app);
    let i = s.inner.lock().unwrap();
    i.oauth.is_some() || i.pat.is_some()
}

/// Token de acesso válido: OAuth (renovando se preciso) ou o PAT.
async fn access_token(app: &AppHandle, force_refresh: bool) -> Result<String, ApiError> {
    let s = shared(app);
    let (oauth, pat) = {
        let i = s.inner.lock().unwrap();
        (i.oauth.clone(), i.pat.clone())
    };
    let Some(seen) = oauth else {
        return pat.ok_or(ApiError::Unauthorized);
    };
    if !force_refresh && !seen.expires_soon() {
        return Ok(seen.access_token);
    }
    let Some(oa) = s.oauth_app.clone() else {
        return Err(ApiError::Unauthorized);
    };
    let _guard = s.refresh_lock.lock().await;
    // Outra tarefa pode ter renovado enquanto esperávamos o lock.
    let current = s.inner.lock().unwrap().oauth.clone();
    let Some(current) = current else {
        return Err(ApiError::Unauthorized);
    };
    if current.refresh_token != seen.refresh_token {
        return Ok(current.access_token);
    }
    match auth::refresh(s.client.http(), &oa, &current.refresh_token).await {
        Ok(new) => {
            let access = new.access_token.clone();
            store_tokens(app, Some(new));
            Ok(access)
        }
        Err(AuthError::Invalid(_)) => {
            store_tokens(app, None);
            Err(ApiError::Unauthorized)
        }
        Err(AuthError::Network(e)) => Err(ApiError::Network(e)),
        Err(AuthError::Other(e)) => Err(ApiError::Http(0, e)),
    }
}

/// Executa uma chamada com token; num 401 com OAuth, renova e tenta mais uma vez.
pub async fn with_token<T, F, Fut>(app: &AppHandle, f: F) -> Result<T, ApiError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<T, ApiError>>,
{
    let token = access_token(app, false).await?;
    let result = f(token).await;
    let is_oauth = shared(app).inner.lock().unwrap().oauth.is_some();
    match result {
        Err(ApiError::Unauthorized) if is_oauth => f(access_token(app, true).await?).await,
        r => r,
    }
}

/// Depois de obter credenciais novas: acha o aparelho e busca o status.
pub async fn finish_login(app: &AppHandle) -> Result<Snapshot, String> {
    {
        let s = shared(app);
        let mut i = s.inner.lock().unwrap();
        i.forget_device();
        i.blocked_until = None;
    }
    sync::set_conn(app, Conn::Loading, None);
    sync::refresh(app, true).await;
    let snap = snapshot(app);
    match snap.conn {
        Conn::Ok => Ok(snap),
        _ => Err(snap
            .message
            .unwrap_or_else(|| tr("Não foi possível conectar", "Could not connect").into())),
    }
}

pub fn not_configured() -> String {
    tr(
        "Login OAuth não configurado neste build",
        "OAuth login is not configured in this build",
    )
    .into()
}

pub fn cancelled() -> String {
    tr("Login cancelado", "Login cancelled").into()
}

pub async fn exchange_code(app: &AppHandle, code: &str) -> Result<(), String> {
    let s = shared(app);
    let oa = s.oauth_app.clone().ok_or_else(not_configured)?;
    let tokens = auth::exchange_code(s.client.http(), &oa, code, auth::REDIRECT_URI)
        .await
        .map_err(|e| e.to_string())?;
    store_tokens(app, Some(tokens));
    // O PAT antigo não é mais usado; não deixa segredo guardado à toa.
    secrets::delete(secrets::PAT);
    s.inner.lock().unwrap().pat = None;
    Ok(())
}

/// Abre a janela de login da SmartThings dentro do app e devolve o código de autorização.
pub async fn login_window_code(app: &AppHandle, oa: &OAuthApp) -> Result<String, String> {
    if let Some(old) = app.get_webview_window("login") {
        let _ = old.close();
    }
    let state = auth::random_state();
    let url: tauri::Url = auth::authorize_url(oa, auth::REDIRECT_URI, &state)
        .parse()
        .map_err(|e| format!("{e}"))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<String, String>>();
    let nav_tx = tx.clone();
    let win = tauri::WebviewWindowBuilder::new(app, "login", tauri::WebviewUrl::External(url))
        .title(tr("Entrar com SmartThings", "Sign in with SmartThings"))
        .inner_size(480.0, 720.0)
        .center()
        .focused(true)
        // Anônima: a sessão da conta Samsung não fica salva no app.
        .incognito(true)
        .on_navigation(move |u| {
            // Intercepta o redirecionamento final: o código nunca sai do app.
            if auth::is_redirect(u) {
                let _ = nav_tx.send(auth::code_from_redirect(u.as_str(), &state));
                return false;
            }
            matches!(u.scheme(), "https" | "about")
        })
        .build()
        .map_err(|e| e.to_string())?;
    win.on_window_event(move |e| {
        if let WindowEvent::Destroyed = e {
            let _ = tx.send(Err(cancelled()));
        }
    });

    let s = shared(app);
    let code = tokio::select! {
        r = rx.recv() => r.unwrap_or_else(|| Err(cancelled())),
        _ = s.login_cancel.notified() => Err(cancelled()),
        _ = tokio::time::sleep(Duration::from_secs(600)) => Err(tr("Tempo esgotado", "Timed out").into()),
    };
    if let Some(w) = app.get_webview_window("login") {
        let _ = w.close();
    }
    code
}
