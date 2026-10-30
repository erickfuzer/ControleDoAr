//! Credenciais: tokens OAuth (com renovação automática) ou PAT.

use crate::api::ApiError;
use crate::auth::{self, AuthError, Tokens};
use crate::core::{Conn, Core, Snapshot};
use crate::i18n::tr;
use crate::{secrets, sync};
use std::future::Future;
use std::sync::Arc;

pub fn store_tokens(core: &Arc<Core>, tokens: Option<Tokens>) {
    match &tokens {
        Some(t) => {
            if let Err(e) = secrets::save(secrets::OAUTH, &serde_json::to_string(t).unwrap_or_default()) {
                eprintln!("could not save tokens to Credential Manager: {e}");
            }
        }
        None => secrets::delete(secrets::OAUTH),
    }
    core.inner.lock().unwrap().oauth = tokens;
}

pub fn has_credentials(core: &Arc<Core>) -> bool {
    let s = core;
    let i = s.inner.lock().unwrap();
    i.oauth.is_some() || i.pat.is_some()
}

/// Token de acesso válido: OAuth (renovando se preciso) ou o PAT.
async fn access_token(core: &Arc<Core>, force_refresh: bool) -> Result<String, ApiError> {
    let s = core;
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
            store_tokens(core, Some(new));
            Ok(access)
        }
        Err(AuthError::Invalid(_)) => {
            store_tokens(core, None);
            Err(ApiError::Unauthorized)
        }
        Err(AuthError::Network(e)) => Err(ApiError::Network(e)),
        Err(AuthError::Other(e)) => Err(ApiError::Http(0, e)),
    }
}

/// Executa uma chamada com token; num 401 com OAuth, renova e tenta mais uma vez.
pub async fn with_token<T, F, Fut>(core: &Arc<Core>, f: F) -> Result<T, ApiError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<T, ApiError>>,
{
    let token = access_token(core, false).await?;
    let result = f(token).await;
    let is_oauth = core.inner.lock().unwrap().oauth.is_some();
    match result {
        Err(ApiError::Unauthorized) if is_oauth => f(access_token(core, true).await?).await,
        r => r,
    }
}

/// Depois de obter credenciais novas: acha o aparelho e busca o status.
pub async fn finish_login(core: &Arc<Core>) -> Result<Snapshot, String> {
    {
        let s = core;
        let mut i = s.inner.lock().unwrap();
        i.forget_device();
        i.blocked_until = None;
    }
    sync::set_conn(core, Conn::Loading, None);
    sync::refresh(core, true).await;
    let snap = core.snapshot();
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

/// Troca o código do login pelos tokens. `redirect` precisa ser o mesmo usado na autorização.
pub async fn exchange_code(core: &Arc<Core>, code: &str, redirect: &str) -> Result<(), String> {
    let s = core;
    let oa = s.oauth_app.clone().ok_or_else(not_configured)?;
    let tokens = auth::exchange_code(s.client.http(), &oa, code, redirect)
        .await
        .map_err(|e| e.to_string())?;
    store_tokens(core, Some(tokens));
    // O PAT antigo não é mais usado; não deixa segredo guardado à toa.
    secrets::delete(secrets::PAT);
    s.inner.lock().unwrap().pat = None;
    Ok(())
}
