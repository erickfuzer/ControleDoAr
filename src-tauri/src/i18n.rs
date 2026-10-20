//! Textos do lado Rust (menu da bandeja, dicas, erros): português se o Windows estiver em
//! português, inglês nos demais casos. O popup faz a mesma escolha no JavaScript.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Pt,
    En,
}

pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> Lang {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    // SAFETY: chamada sem ponteiros.
    let id = unsafe { GetUserDefaultUILanguage() };
    // Idioma primário nos 10 bits de baixo; 0x16 = português.
    if id & 0x3ff == 0x16 {
        Lang::Pt
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn detect() -> Lang {
    match std::env::var("LANG") {
        Ok(l) if l.starts_with("pt") => Lang::Pt,
        _ => Lang::En,
    }
}

/// Escolhe o texto do idioma atual.
pub fn tr(pt: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Pt => pt,
        Lang::En => en,
    }
}

pub fn mode_label(mode: &str) -> String {
    let s = match mode {
        "auto" => tr("Auto", "Auto"),
        "cool" => tr("Fresco", "Cool"),
        "dry" => tr("Seco", "Dry"),
        "wind" => tr("Ventilar", "Fan"),
        "heat" => tr("Aquecer", "Heat"),
        other => return other.to_string(),
    };
    s.to_string()
}
