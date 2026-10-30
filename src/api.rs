//! Cliente mínimo da API SmartThings.

use crate::i18n::tr;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

const BASE: &str = "https://api.smartthings.com/v1";

#[derive(Debug)]
pub enum ApiError {
    /// Token inválido ou expirado (HTTP 401).
    Unauthorized,
    /// Limite de requisições (HTTP 429); segundos até poder tentar de novo.
    RateLimited(u64),
    Http(u16, String),
    Network(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "{}", tr("Login inválido ou expirado", "Invalid or expired login")),
            ApiError::RateLimited(s) => {
                write!(
                    f,
                    "{} {s} s",
                    tr("Limite da API atingido, aguarde", "API rate limit reached, wait")
                )
            }
            ApiError::Http(code, body) => write!(f, "{} {code}: {body}", tr("Erro", "Error")),
            ApiError::Network(e) => write!(f, "{}: {e}", tr("Sem conexão", "No connection")),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub label: String,
    pub is_ac: bool,
    #[serde(skip)]
    pub location_id: Option<String>,
    #[serde(skip)]
    pub room_id: Option<String>,
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(6))
            .build()
            .expect("cliente HTTP");
        Self { http }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    async fn check(resp: reqwest::Response) -> Result<Value, ApiError> {
        let status = resp.status();
        if status.as_u16() == 401 {
            return Err(ApiError::Unauthorized);
        }
        if status.as_u16() == 429 {
            return Err(ApiError::RateLimited(retry_after(resp.headers())));
        }
        let text = resp.text().await.map_err(|e| ApiError::Network(e.to_string()))?;
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or(text);
            return Err(ApiError::Http(status.as_u16(), msg));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn get(&self, token: &str, path: &str) -> Result<Value, ApiError> {
        let resp = self
            .http
            .get(format!("{BASE}{path}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        Self::check(resp).await
    }

    pub async fn devices(&self, token: &str) -> Result<Vec<DeviceInfo>, ApiError> {
        let v = self.get(token, "/devices").await?;
        let items = v["items"].as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .map(|d| {
                let is_ac = d["components"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|c| c["capabilities"].as_array().cloned().unwrap_or_default())
                    .any(|c| c["id"] == "airConditionerMode");
                DeviceInfo {
                    id: d["deviceId"].as_str().unwrap_or_default().to_string(),
                    label: d["label"].as_str().or(d["name"].as_str()).unwrap_or("?").to_string(),
                    is_ac,
                    location_id: d["locationId"].as_str().map(String::from),
                    room_id: d["roomId"].as_str().map(String::from),
                }
            })
            .collect())
    }

    /// "Casa - Quarto 1", como no app SmartThings. Precisa do escopo `r:locations:*`.
    pub async fn place(&self, token: &str, location_id: &str, room_id: Option<&str>) -> Result<String, ApiError> {
        let loc = self.get(token, &format!("/locations/{location_id}")).await?;
        let mut name = loc["name"].as_str().unwrap_or_default().to_string();
        if let Some(room) = room_id {
            let r = self
                .get(token, &format!("/locations/{location_id}/rooms/{room}"))
                .await?;
            if let Some(room_name) = r["name"].as_str() {
                if !name.is_empty() {
                    name.push_str(" - ");
                }
                name.push_str(room_name);
            }
        }
        Ok(name)
    }

    pub async fn status(&self, token: &str, device: &str) -> Result<Value, ApiError> {
        self.get(token, &format!("/devices/{device}/status")).await
    }

    pub async fn health(&self, token: &str, device: &str) -> Result<bool, ApiError> {
        let v = self.get(token, &format!("/devices/{device}/health")).await?;
        Ok(v["state"].as_str() != Some("OFFLINE"))
    }

    pub async fn commands(&self, token: &str, device: &str, commands: Vec<Value>) -> Result<(), ApiError> {
        let resp = self
            .http
            .post(format!("{BASE}/devices/{device}/commands"))
            .bearer_auth(token)
            .json(&json!({ "commands": commands }))
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let v = Self::check(resp).await?;
        // A nuvem responde 200 mesmo quando um comando individual falha.
        if let Some(failed) = v["results"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["status"] == "FAILED")
        {
            return Err(ApiError::Http(
                200,
                format!("{}: {failed}", tr("comando recusado", "command rejected")),
            ));
        }
        Ok(())
    }
}

/// Quanto esperar depois de um 429. `X-RateLimit-Reset` da SmartThings vem em milissegundos.
fn retry_after(h: &reqwest::header::HeaderMap) -> u64 {
    let num = |name: &str| h.get(name)?.to_str().ok()?.trim().parse::<u64>().ok();
    num("retry-after")
        .or_else(|| num("x-ratelimit-reset").map(|ms| ms.div_ceil(1000)))
        .unwrap_or(30)
        .clamp(5, 300)
}

/// Monta um comando para o componente principal.
pub fn cmd(capability: &str, command: &str, args: Vec<Value>) -> Value {
    let mut c = json!({ "component": "main", "capability": capability, "command": command });
    if !args.is_empty() {
        c["arguments"] = Value::Array(args);
    }
    c
}

/// Comando `execute` com opção do protocolo OCF da Samsung (ver docs/API-NOTES.md).
pub fn exec_option(option: &str) -> Value {
    cmd(
        "execute",
        "execute",
        vec![json!("mode/vs/0"), json!({ "x.com.samsung.da.options": [option] })],
    )
}
