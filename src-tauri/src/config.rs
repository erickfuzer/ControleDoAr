//! Configuração do usuário (AppData). Não guarda credenciais: essas ficam no Gerenciador de Credenciais.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub device_id: Option<String>,
    /// Versões antigas gravavam o PAT aqui em texto puro. Só é lido, para migrar.
    #[serde(default, rename = "token", skip_serializing)]
    pub legacy_token: Option<String>,
}

impl Config {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }
}

/// Só em desenvolvimento: PAT de `SMARTTHINGS_PAT` ou do `.env` na raiz do projeto.
pub fn dev_token() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    if let Ok(t) = std::env::var("SMARTTHINGS_PAT") {
        return Some(t);
    }
    let env = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.env");
    std::fs::read_to_string(env)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("SMARTTHINGS_PAT=").map(|t| t.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn token_antigo_e_lido_mas_nunca_gravado() {
        let c: Config = serde_json::from_str(r#"{"token":"segredo","device_id":"d1"}"#).unwrap();
        assert_eq!(c.legacy_token.as_deref(), Some("segredo"));
        let out = serde_json::to_string(&c).unwrap();
        assert!(!out.contains("segredo") && out.contains("d1"));
    }
}
