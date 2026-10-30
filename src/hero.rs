//! Fundo do banner "ligado": degradê na cor do modo, ondas claras e cantos arredondados.
//!
//! O renderizador por software do Slint não arredonda degradês nem desenha bem curvas
//! sobrepostas, então o app pinta a imagem pixel a pixel, no tamanho exato da tela.

/// Tamanho lógico do banner (o popup tem 380 de largura e 10 de margem de cada lado).
pub const WIDTH: f32 = 360.0;
pub const HEIGHT: f32 = 100.0;
const RADIUS: f32 = 26.0;
/// As ondas, no mesmo sistema de coordenadas do SVG original (360 × 100).
const WAVES: [[(f32, f32); 7]; 2] = [
    // M0 70 C 80 40, 150 95, 230 60 S 330 30, 360 45
    [
        (0.0, 70.0),
        (80.0, 40.0),
        (150.0, 95.0),
        (230.0, 60.0),
        (310.0, 25.0),
        (330.0, 30.0),
        (360.0, 45.0),
    ],
    // M0 85 C 90 60, 170 100, 250 78 S 330 60, 360 70
    [
        (0.0, 85.0),
        (90.0, 60.0),
        (170.0, 100.0),
        (250.0, 78.0),
        (330.0, 56.0),
        (330.0, 60.0),
        (360.0, 70.0),
    ],
];
const WAVE_ALPHA: f32 = 0.09;

fn colors(mode: &str) -> [[f32; 3]; 3] {
    let hex = |v: u32| {
        [
            ((v >> 16) & 255) as f32 / 255.0,
            ((v >> 8) & 255) as f32 / 255.0,
            (v & 255) as f32 / 255.0,
        ]
    };
    let [a, b, c] = match mode {
        "heat" => [0xd8505a, 0xec7c48, 0xf5b45a],
        "dry" => [0x2f8a8f, 0x3fb3a8, 0x7dd6c0],
        "wind" => [0x3d8a52, 0x5fb466, 0xa3d47c],
        "auto" => [0x6a52d1, 0x8d68de, 0xc58ae6],
        _ => [0x5b5fd6, 0x4b8fe8, 0x5cc6e3], // cool
    };
    [hex(a), hex(b), hex(c)]
}

fn cubic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

/// Altura da onda (em pixels) para cada coluna.
fn wave_tops(wave: &[(f32, f32); 7], w: usize, h: usize) -> Vec<f32> {
    let mut pts = Vec::with_capacity(400);
    for seg in [
        [wave[0], wave[1], wave[2], wave[3]],
        [wave[3], wave[4], wave[5], wave[6]],
    ] {
        for i in 0..=200 {
            let (x, y) = cubic(seg[0], seg[1], seg[2], seg[3], i as f32 / 200.0);
            pts.push((x / 360.0 * w as f32, y / 100.0 * h as f32));
        }
    }
    (0..w)
        .map(|col| {
            let x = col as f32 + 0.5;
            let i = pts.partition_point(|p| p.0 < x).clamp(1, pts.len() - 1);
            let (a, b) = (pts[i - 1], pts[i]);
            let t = if b.0 > a.0 { (x - a.0) / (b.0 - a.0) } else { 0.0 };
            a.1 + (b.1 - a.1) * t
        })
        .collect()
}

/// Imagem RGBA (alfa não pré-multiplicado) com `w` × `h` pixels físicos.
pub fn render(w: u32, h: u32, scale: f32, mode: &str) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let (wu, hu) = (w as usize, h as usize);
    let [c1, c2, c3] = colors(mode);
    let tops: Vec<Vec<f32>> = WAVES.iter().map(|wv| wave_tops(wv, wu, hu)).collect();
    let r = RADIUS * scale;
    let (wf, hf) = (w as f32, h as f32);
    // Degradê de 105° como no CSS: quase da esquerda para a direita, descendo um pouco.
    let (sin, cos) = (105f32.to_radians().sin(), 105f32.to_radians().cos());
    let len = (wf * sin).abs() + (hf * cos).abs();

    let mut out = vec![0u8; wu * hu * 4];
    for y in 0..hu {
        for x in 0..wu {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Cobertura do retângulo arredondado (antisserrilhado pela distância à borda).
            let qx = (px - wf / 2.0).abs() - (wf / 2.0 - r);
            let qy = (py - hf / 2.0).abs() - (hf / 2.0 - r);
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let t = (((px - wf / 2.0) * sin - (py - hf / 2.0) * cos) / len + 0.5).clamp(0.0, 1.0);
            let (a, b, f) = if t < 0.48 {
                (c1, c2, t / 0.48)
            } else {
                (c2, c3, (t - 0.48) / 0.52)
            };
            let mut rgb = [0.0f32; 3];
            for i in 0..3 {
                rgb[i] = a[i] + (b[i] - a[i]) * f;
            }
            // Ondas brancas translúcidas, com a borda de cima suavizada.
            for top in &tops {
                let wave = (py + 0.5 - top[x]).clamp(0.0, 1.0) * WAVE_ALPHA;
                for c in &mut rgb {
                    *c += (1.0 - *c) * wave;
                }
            }
            let o = (y * wu + x) * 4;
            out[o] = (rgb[0] * 255.0).round() as u8;
            out[o + 1] = (rgb[1] * 255.0).round() as u8;
            out[o + 2] = (rgb[2] * 255.0).round() as u8;
            out[o + 3] = (cover * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn cantos_transparentes_e_centro_opaco() {
        let (w, h) = (360u32, 100u32);
        let img = super::render(w, h, 1.0, "cool");
        let alpha = |x: u32, y: u32| img[((y * w + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(w - 1, h - 1), 0);
        assert_eq!(alpha(w / 2, h / 2), 255);
    }
}
