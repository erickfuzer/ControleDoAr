fn main() {
    // Interface (ui/*.slint) compilada para Rust.
    slint_build::compile("ui/app.slint").expect("falha ao compilar a interface Slint");

    // Credenciais do app OAuth: vêm do ambiente (CI) ou do .env local, nunca do repositório.
    println!("cargo:rerun-if-changed=.env");
    let dotenv = std::fs::read_to_string(".env").unwrap_or_default();
    for key in ["ARCOND_CLIENT_ID", "ARCOND_CLIENT_SECRET", "ARCOND_REDIRECT_URI"] {
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

    // Ícone, versão e manifesto do executável.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Controle do Ar")
            .set("FileDescription", "Controle do Ar")
            .set("CompanyName", "Erick Fuzer")
            .set("LegalCopyright", "Copyright (c) 2026 Erick Cassol Fuzer. Licença MIT.")
            .set_manifest_file("assets/app.manifest");
        res.compile().expect("falha ao gerar os recursos do Windows");
    }
}
