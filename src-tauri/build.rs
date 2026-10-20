fn main() {
    // Credenciais do app OAuth: vêm do ambiente (CI) ou do .env local, nunca do repositório.
    println!("cargo:rerun-if-changed=../.env");
    let dotenv = std::fs::read_to_string("../.env").unwrap_or_default();
    for key in ["ARCOND_CLIENT_ID", "ARCOND_CLIENT_SECRET"] {
        println!("cargo:rerun-if-env-changed={key}");
        let value = std::env::var(key).ok().or_else(|| {
            dotenv
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{key}=")).map(|v| v.trim().to_string()))
        });
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            println!("cargo:rustc-env={key}={v}");
        }
    }
    tauri_build::build()
}
