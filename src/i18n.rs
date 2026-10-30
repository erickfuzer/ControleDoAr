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

/// Rótulo da intensidade do vento; valores de outros modelos aparecem como vieram.
pub fn fan_label(v: &str) -> String {
    let s = match v {
        "auto" => tr("Automático", "Auto"),
        "low" => tr("Baixo", "Low"),
        "medium" => tr("Médio", "Medium"),
        "high" => tr("Alto", "High"),
        "turbo" => tr("Turbo", "Turbo"),
        other => return other.to_string(),
    };
    s.to_string()
}

pub fn osc_label(v: &str) -> String {
    let s = match v {
        "fixed" => tr("Fixo", "Fixed"),
        "vertical" => tr("Vertical", "Vertical"),
        "horizontal" => tr("Horizontal", "Horizontal"),
        "all" => tr("Todas as direções", "All directions"),
        other => return other.to_string(),
    };
    s.to_string()
}

/// Título do banner quando ligado ("Resfriamento").
pub fn state_title(mode: &str) -> String {
    let s = match mode {
        "cool" => tr("Resfriamento", "Cooling"),
        "heat" => tr("Aquecimento", "Heating"),
        "dry" => tr("Desumidificação", "Drying"),
        "wind" => tr("Ventilação", "Fan"),
        "auto" => tr("Automático", "Auto"),
        other => return mode_label(other),
    };
    s.to_string()
}

pub fn filter_label(v: &str) -> String {
    let s = match v {
        "normal" => tr("OK", "OK"),
        "wash" => tr("Lavar", "Wash"),
        "replace" => tr("Trocar", "Replace"),
        other => return other.to_string(),
    };
    s.to_string()
}

/// ["a", "b", "c"] → "A, b e c"
pub fn list_text(items: &[&str]) -> String {
    let text = match items {
        [] => return String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} {} {last}", rest.join(", "), tr("e", "and")),
    };
    let mut c = text.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_em_texto() {
        assert_eq!(list_text(&[]), "");
        assert_eq!(list_text(&["luz apagada"]), "Luz apagada");
        let e = tr("e", "and");
        assert_eq!(list_text(&["a", "b", "c"]), format!("A, b {e} c"));
    }
}
