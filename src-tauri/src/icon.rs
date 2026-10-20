//! Desenha o ícone da bandeja no tamanho exato que o Windows usa (16 px a 100% de escala),
//! com a temperatura ocupando quase o ícone inteiro.

use ab_glyph::{Font, FontVec, PxScale, Rect, ScaleFont, VariableFont};
use std::sync::OnceLock;

/// Bahnschrift (fonte de painéis/placas do Windows) em negrito condensado deixa os dígitos
/// mais altos no mesmo espaço; Segoe UI Bold e Arial Bold ficam de reserva.
fn font() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(|| {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        let read = |f: &str| {
            let bytes = std::fs::read(format!("{dir}\\Fonts\\{f}")).ok()?;
            FontVec::try_from_vec(bytes).ok()
        };
        if let Some(mut f) = read("bahnschrift.ttf") {
            f.set_variation(b"wght", 700.0);
            f.set_variation(b"wdth", 75.0);
            return Some(f);
        }
        ["segoeuib.ttf", "arialbd.ttf"].iter().find_map(|f| read(f))
    })
    .as_ref()
}

/// Tamanho do ícone pequeno do sistema (16 a 100%, 20 a 125%, 24 a 150%...).
pub fn tray_size() -> u32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
        // SAFETY: chamada sem ponteiros.
        let s = unsafe { GetSystemMetrics(SM_CXSMICON) };
        if s > 0 {
            return (s as u32).clamp(16, 64);
        }
    }
    32
}

pub fn mode_color(mode: &str) -> [u8; 3] {
    match mode {
        "cool" => [0x2f, 0x7f, 0xf0],
        "heat" => [0xf0, 0x6a, 0x1c],
        "dry" => [0x12, 0xa1, 0x95],
        "wind" => [0x4f, 0xa0, 0x2e],
        _ => [0x86, 0x5c, 0xf0], // auto
    }
}

struct Canvas {
    size: u32,
    px: Vec<[f32; 4]>, // rgb + alpha, não pré-multiplicado
}

impl Canvas {
    fn new(size: u32) -> Self {
        Self {
            size,
            px: vec![[0.0; 4]; (size * size) as usize],
        }
    }

    fn blend(&mut self, x: i32, y: i32, rgb: [u8; 3], cov: f32) {
        let n = self.size as i32;
        if x < 0 || y < 0 || x >= n || y >= n || cov <= 0.0 {
            return;
        }
        let p = &mut self.px[(y * n + x) as usize];
        let a = cov.min(1.0);
        let out_a = a + p[3] * (1.0 - a);
        for i in 0..3 {
            let c = rgb[i] as f32 / 255.0;
            p[i] = if out_a > 0.0 {
                (c * a + p[i] * p[3] * (1.0 - a)) / out_a
            } else {
                0.0
            };
        }
        p[3] = out_a;
    }

    /// Retângulo arredondado ocupando o ícone inteiro.
    fn rrect(&mut self, rgb: [u8; 3], radius: f32) {
        let half = self.size as f32 / 2.0;
        for y in 0..self.size as i32 {
            for x in 0..self.size as i32 {
                let qx = (x as f32 + 0.5 - half).abs() - (half - radius);
                let qy = (y as f32 + 0.5 - half).abs() - (half - radius);
                let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
                self.blend(x, y, rgb, (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// Escreve `text` centralizado, no maior tamanho que caiba em `max_w` × `max_h`.
    fn text(&mut self, text: &str, rgb: [u8; 3], max_w: f32, max_h: f32) {
        let Some(font) = font() else { return };
        let layout = |px: f32| {
            let scale = PxScale::from(px);
            let sf = font.as_scaled(scale);
            let mut x = 0.0;
            let mut outlines = Vec::new();
            for ch in text.chars() {
                let id = font.glyph_id(ch);
                let g = id.with_scale_and_position(scale, ab_glyph::point(x, sf.ascent()));
                x += sf.h_advance(id);
                outlines.extend(font.outline_glyph(g));
            }
            let bounds = outlines.iter().map(|o| o.px_bounds()).reduce(|a, b| Rect {
                min: ab_glyph::point(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
                max: ab_glyph::point(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
            });
            (outlines, bounds)
        };
        // Mede num tamanho de referência e escala para caber.
        let Some(r) = layout(100.0).1 else { return };
        let px = 100.0 * (max_w / r.width()).min(max_h / r.height());
        let (outlines, Some(b)) = layout(px) else { return };
        let n = self.size as f32;
        let off_x = ((n - b.width()) / 2.0 - b.min.x).round();
        let off_y = ((n - b.height()) / 2.0 - b.min.y).round();
        for o in &outlines {
            let ob = o.px_bounds();
            o.draw(|gx, gy, c| {
                self.blend(
                    (ob.min.x + off_x) as i32 + gx as i32,
                    (ob.min.y + off_y) as i32 + gy as i32,
                    rgb,
                    c,
                );
            });
        }
    }

    fn into_rgba(self) -> Vec<u8> {
        self.px
            .iter()
            .flat_map(|p| p.map(|v| (v * 255.0).round() as u8))
            .collect()
    }
}

/// Ícone no tamanho do sistema. `setpoint = None` desenha "--" (sem conexão ou sem login).
pub fn render(power: bool, mode: &str, setpoint: Option<i64>) -> (Vec<u8>, u32) {
    let size = tray_size();
    (render_at(size, power, mode, setpoint), size)
}

pub fn render_at(size: u32, power: bool, mode: &str, setpoint: Option<i64>) -> Vec<u8> {
    let mut c = Canvas::new(size);
    let n = size as f32;
    let text = setpoint.map(|t| t.to_string()).unwrap_or_else(|| "--".into());
    if power && setpoint.is_some() {
        // Ligado: quadrado na cor do modo com o número em branco.
        c.rrect(mode_color(mode), n * 0.2);
        c.text(&text, [255, 255, 255], n * 0.84, n * 0.72);
    } else if setpoint.is_some() {
        // Desligado: só o número, cinza claro, o maior possível.
        c.text(&text, [0xc8, 0xc8, 0xc8], n * 0.96, n * 0.80);
    } else {
        c.text(&text, [0x9a, 0x9a, 0x9a], n * 0.62, n * 0.62);
    }
    c.into_rgba()
}

#[cfg(test)]
mod tests {
    /// Gera prévias em `ICON_PREVIEW_DIR` (rgba cru): `cargo test --lib -- --ignored preview`.
    #[test]
    #[ignore]
    fn preview() {
        let dir = std::env::var("ICON_PREVIEW_DIR").expect("ICON_PREVIEW_DIR");
        let cases = [
            ("cool19", true, "cool", Some(19)),
            ("heat30", true, "heat", Some(30)),
            ("dry24", true, "dry", Some(24)),
            ("off24", false, "cool", Some(24)),
            ("none", false, "cool", None),
        ];
        for size in [16u32, 20, 24, 32] {
            for (name, on, mode, sp) in cases {
                let rgba = super::render_at(size, on, mode, sp);
                std::fs::write(format!("{dir}/{size}_{name}.rgba"), rgba).unwrap();
            }
        }
    }
}
