//! Login da SmartThings pelo navegador do sistema (funciona com conta Google).
//!
//! O app escuta numa porta livre de 127.0.0.1 e coloca o número dela no início do `state`.
//! A SmartThings devolve o login para a página-ponte (docs/callback.html), que repassa o código
//! para essa porta. O `state` completo é conferido antes de usar o código.

use crate::core::Core;
use crate::i18n::tr;
use crate::{auth, session, winutil};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TIMEOUT: Duration = Duration::from_secs(600);

/// Login normal: abre o navegador e espera a volta pela ponte.
pub async fn browser(core: &Arc<Core>) -> Result<(), String> {
    let oa = core.oauth_app.clone().ok_or_else(session::not_configured)?;
    core.login_cancel.notify_waiters();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = format!("{port}.{}", auth::random_state());
    winutil::open_url(&auth::authorize_url(&oa, auth::BRIDGE_REDIRECT, &state))?;
    let code = wait_for_code(&listener, &state, &core.login_cancel).await?;
    session::exchange_code(core, &code, auth::BRIDGE_REDIRECT).await
}

/// Plano B: abre o login com o redirecionamento do httpbin; o usuário cola o endereço final.
pub fn manual_start(core: &Core) -> Result<(), String> {
    let oa = core.oauth_app.clone().ok_or_else(session::not_configured)?;
    core.login_cancel.notify_waiters();
    let state = auth::random_state();
    *core.manual_state.lock().unwrap() = Some(state.clone());
    winutil::open_url(&auth::authorize_url(&oa, auth::MANUAL_REDIRECT, &state))
}

pub async fn manual_finish(core: &Arc<Core>, input: &str) -> Result<(), String> {
    let code = auth::extract_code(input).ok_or_else(|| {
        tr(
            "Não encontrei o código no texto colado",
            "No code found in the pasted text",
        )
        .to_string()
    })?;
    let expected = core.manual_state.lock().unwrap().clone();
    if let (Some(exp), Some(i)) = (expected, input.find("state")) {
        if !input[i..].contains(&exp) {
            return Err(tr(
                "Esse endereço é de outra tentativa de login",
                "That address is from another login attempt",
            )
            .into());
        }
    }
    session::exchange_code(core, &code, auth::MANUAL_REDIRECT).await
}

async fn wait_for_code(listener: &TcpListener, state: &str, cancel: &tokio::sync::Notify) -> Result<String, String> {
    let deadline = tokio::time::sleep(TIMEOUT);
    tokio::pin!(deadline);
    loop {
        let accepted = tokio::select! {
            _ = &mut deadline => return Err(tr("Tempo esgotado", "Timed out").into()),
            _ = cancel.notified() => return Err(session::cancelled()),
            a = listener.accept() => a,
        };
        let Ok((mut sock, _)) = accepted else { continue };
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("");
        if !path.starts_with("/callback?") {
            // favicon e afins
            let _ = sock
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            continue;
        }
        let url = format!("http://127.0.0.1{path}");
        // Outro programa do PC pode bater nesta porta; sem o `state` certo, não encerra o login.
        if !state_matches(&url, state) {
            let _ = sock.write_all(page(false).as_bytes()).await;
            let _ = sock.shutdown().await;
            continue;
        }
        let result = auth::code_from_redirect(&url, state);
        let _ = sock.write_all(page(result.is_ok()).as_bytes()).await;
        let _ = sock.shutdown().await;
        return result;
    }
}

fn state_matches(url: &str, state: &str) -> bool {
    reqwest::Url::parse(url)
        .map(|u| u.query_pairs().any(|(k, v)| k == "state" && v == state))
        .unwrap_or(false)
}

/// Página que o navegador mostra no fim. Não repete nada que veio na URL.
fn page(ok: bool) -> String {
    let (title, text) = if ok {
        (
            tr("Pronto!", "Done!"),
            tr(
                "Login feito. Pode fechar esta aba e voltar para o Controle do Ar.",
                "Signed in. You can close this tab and go back to Air Remote.",
            ),
        )
    } else {
        (
            tr("Não deu certo", "Something went wrong"),
            tr("Abra o login de novo pelo app.", "Start the login again from the app."),
        )
    };
    let html = format!(
        "<!doctype html><html><meta charset=utf-8><meta name=viewport content='width=device-width'>\
         <title>Controle do Ar</title>\
         <body style=\"margin:0;height:100vh;display:grid;place-items:center;background:#000;color:#f4f4f6;\
         font:15px 'Segoe UI',system-ui,sans-serif;text-align:center\">\
         <div><h1 style=\"font-size:20px\">{title}</h1><p style=\"color:#a3a3aa\">{text}</p></div>"
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
}

#[cfg(test)]
mod tests {
    use crate::auth;

    /// O `state` precisa passar pela validação da página-ponte (docs/callback.html):
    /// /^(\d{4,5})\.[A-Za-z0-9]{16,128}$/
    #[test]
    fn state_aceito_pela_ponte() {
        for port in [1024u16, 50123, 65535] {
            let state = format!("{port}.{}", auth::random_state());
            let (p, rest) = state.split_once('.').unwrap();
            assert!((4..=5).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()));
            assert!((16..=128).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }

    #[test]
    fn state_errado_nao_encerra_o_login() {
        assert!(super::state_matches(
            "http://127.0.0.1/callback?code=x&state=5000.abc",
            "5000.abc"
        ));
        assert!(!super::state_matches(
            "http://127.0.0.1/callback?code=x&state=5000.xyz",
            "5000.abc"
        ));
        assert!(!super::state_matches("http://127.0.0.1/callback?code=x", "5000.abc"));
    }

    #[test]
    fn pagina_final_nao_repete_dados_da_url() {
        let html = super::page(false);
        assert!(html.starts_with("HTTP/1.1 200 OK"));
        assert!(!html.contains("code=") && !html.contains("state"));
    }
}
