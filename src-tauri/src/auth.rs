//! Login OAuth (authorization code) com a SmartThings.
//!
//! O refresh token é de uso único: cada renovação devolve um par novo, que precisa ser salvo na hora.

use crate::i18n::tr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A SmartThings só aceita redirecionamento HTTPS (recusa http://localhost com 403).
/// No login dentro do app a navegação para cá é interceptada antes de sair do app;
/// no modo manual (navegador do sistema) a página mostra o código para o usuário colar.
pub const REDIRECT_URI: &str = "https://httpbin.org/get";
const SCOPES: &str = "r:devices:* x:devices:* r:locations:*";
const AUTHORIZE_URL: &str = "https://api.smartthings.com/oauth/authorize";
const TOKEN_URL: &str = "https://api.smartthings.com/oauth/token";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Credenciais do app OAuth, embutidas no build a partir do `.env` (ver build.rs).
#[derive(Clone)]
pub struct OAuthApp {
    pub id: String,
    pub secret: String,
}

impl OAuthApp {
    pub fn builtin() -> Option<Self> {
        Some(Self {
            id: option_env!("ARCOND_CLIENT_ID")?.to_string(),
            secret: option_env!("ARCOND_CLIENT_SECRET")?.to_string(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix, em segundos.
    pub expires_at: u64,
}

impl Tokens {
    /// Renova com folga de 10 min antes de expirar.
    pub fn expires_soon(&self) -> bool {
        now_secs() + 600 >= self.expires_at
    }
}

#[derive(Debug)]
pub enum AuthError {
    /// Código ou refresh token recusado: precisa entrar de novo.
    Invalid(String),
    Network(String),
    Other(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Invalid(m) => write!(f, "{}: {m}", tr("Autorização recusada", "Authorization refused")),
            AuthError::Network(m) => write!(f, "{}: {m}", tr("Sem conexão", "No connection")),
            AuthError::Other(m) => write!(f, "{m}"),
        }
    }
}

pub fn random_state() -> String {
    use std::hash::{BuildHasher, RandomState};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let a = RandomState::new().hash_one(seed);
    let b = RandomState::new().hash_one(a);
    format!("{a:016x}{b:016x}")
}

pub fn authorize_url(app: &OAuthApp, redirect: &str, state: &str) -> String {
    reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("client_id", app.id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect),
            ("scope", SCOPES),
            ("state", state),
        ],
    )
    .map(|u| u.to_string())
    .unwrap_or_default()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

async fn token_request(http: &reqwest::Client, app: &OAuthApp, form: &[(&str, &str)]) -> Result<Tokens, AuthError> {
    let resp = http
        .post(TOKEN_URL)
        .basic_auth(&app.id, Some(&app.secret))
        .form(form)
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        let t: TokenResponse = serde_json::from_str(&text).map_err(|e| {
            AuthError::Other(format!(
                "{}: {e}",
                tr("Resposta inválida do login", "Invalid login response")
            ))
        })?;
        return Ok(Tokens {
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            expires_at: now_secs() + t.expires_in,
        });
    }
    // Só descarta o login quando a SmartThings diz que o token/código não vale mais;
    // outros erros (instabilidade, 5xx) não podem apagar a sessão.
    if status.as_u16() == 401 || text.contains("invalid_grant") {
        Err(AuthError::Invalid(text))
    } else {
        Err(AuthError::Other(format!(
            "{} {status}: {text}",
            tr("Erro no login", "Login error")
        )))
    }
}

pub async fn exchange_code(
    http: &reqwest::Client,
    app: &OAuthApp,
    code: &str,
    redirect: &str,
) -> Result<Tokens, AuthError> {
    token_request(
        http,
        app,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &app.id),
            ("redirect_uri", redirect),
        ],
    )
    .await
}

pub async fn refresh(http: &reqwest::Client, app: &OAuthApp, refresh_token: &str) -> Result<Tokens, AuthError> {
    token_request(
        http,
        app,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &app.id),
        ],
    )
    .await
}

/// A navegação é exatamente o redirecionamento cadastrado (https://httpbin.org/get)?
pub fn is_redirect(u: &reqwest::Url) -> bool {
    u.scheme() == "https" && u.host_str() == Some("httpbin.org") && u.path() == "/get"
}

/// Lê o código do endereço de redirecionamento, conferindo o `state`.
pub fn code_from_redirect(url: &str, state: &str) -> Result<String, String> {
    let params: HashMap<String, String> = reqwest::Url::parse(url)
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default();
    if let Some(err) = params.get("error") {
        return Err(format!("{} ({err})", tr("Autorização negada", "Authorization denied")));
    }
    if params.get("state").map(String::as_str) != Some(state) {
        return Err(tr("Resposta de login inválida", "Invalid login response").into());
    }
    params
        .get("code")
        .cloned()
        .ok_or_else(|| tr("A SmartThings não enviou o código", "SmartThings did not send the code").to_string())
}

/// Aceita o endereço completo da página do httpbin, o JSON dela ou só o código.
pub fn extract_code(input: &str) -> Option<String> {
    let s = input.trim();
    let take_until = |rest: &str| {
        let end = rest
            .find(|c: char| c == '&' || c == '"' || c == '#' || c.is_whitespace())
            .unwrap_or(rest.len());
        Some(rest[..end].to_string()).filter(|c| !c.is_empty())
    };
    if let Some(i) = s.find("code=") {
        return take_until(&s[i + 5..]);
    }
    if let Some(i) = s.find("\"code\"") {
        let rest = s[i + 6..].trim_start_matches(|c: char| c == ':' || c == '"' || c.is_whitespace());
        return take_until(rest);
    }
    (!s.is_empty() && !s.contains(char::is_whitespace)).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::{code_from_redirect, extract_code};

    #[test]
    fn so_intercepta_o_redirecionamento_exato() {
        let u = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(super::is_redirect(&u("https://httpbin.org/get?code=x&state=y")));
        assert!(!super::is_redirect(&u("http://httpbin.org/get?code=x")));
        assert!(!super::is_redirect(&u("https://httpbin.org.evil.com/get?code=x")));
        assert!(!super::is_redirect(&u("https://httpbin.org/get-outra?code=x")));
    }

    #[test]
    fn le_redirecionamento() {
        let u = "https://httpbin.org/get?code=Ab12&state=s1";
        assert_eq!(code_from_redirect(u, "s1").as_deref(), Ok("Ab12"));
        assert!(code_from_redirect(u, "outro").is_err());
        assert!(code_from_redirect("https://httpbin.org/get?error=access_denied&state=s1", "s1").is_err());
    }

    #[test]
    fn extrai_codigo() {
        assert_eq!(
            extract_code("https://httpbin.org/get?code=Ab12&state=x").as_deref(),
            Some("Ab12")
        );
        assert_eq!(
            extract_code("{\"args\": {\"code\": \"Zz9\", \"state\": \"x\"}}").as_deref(),
            Some("Zz9")
        );
        assert_eq!(extract_code("  Qw3  ").as_deref(), Some("Qw3"));
        assert_eq!(extract_code("").as_deref(), None);
    }
}
