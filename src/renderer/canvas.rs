//! Framebuffer 320x240 com primitivas anti-aliased.
//!
//! A composição acontece em 0x00RRGGBB (u32) para blending preciso; ao final o
//! quadro é convertido de uma vez para RGB565 big-endian (153.600 bytes).

use crate::driver::{FRAME_BYTES, HEIGHT, PIXELS, WIDTH};

/// Conversão escalar exigida pelo protocolo: RGB888 → RGB565 big-endian.
#[allow(dead_code)] // referência do protocolo; a conversão em lote está em `convert_frame`
#[inline(always)]
pub fn rgb888_to_rgb565_be(r: u8, g: u8, b: u8) -> (u8, u8) {
    let val: u16 = (((r as u16) & 0xF8) << 8) | (((g as u16) & 0xFC) << 3) | ((b as u16) >> 3);
    ((val >> 8) as u8, (val & 0xFF) as u8)
}

#[inline(always)]
pub fn u32_to_rgb565(p: u32) -> u16 {
    (((p >> 8) & 0xF800) | ((p >> 5) & 0x07E0) | ((p >> 3) & 0x001F)) as u16
}

#[inline(always)]
pub fn rgb565_to_u32(v: u16) -> u32 {
    let v = v as u32;
    let r = (v >> 11) & 0x1F;
    let g = (v >> 5) & 0x3F;
    let b = v & 0x1F;
    // Replica os bits altos nos baixos para cobrir 0..=255 por inteiro.
    (((r << 3) | (r >> 2)) << 16) | (((g << 2) | (g >> 4)) << 8) | ((b << 3) | (b >> 2))
}

#[inline(always)]
fn convert_chunked(src: &[u32], dst: &mut [u8]) {
    let n = src.len().min(dst.len() / 2);
    let out = dst.as_mut_ptr() as *mut u16;
    // Laço simples sem dependências: o LLVM vetoriza (SSE2 por padrão,
    // AVX2 na variante abaixo) — empacota 8/16 pixels por iteração.
    for (i, &p) in src[..n].iter().enumerate() {
        // SAFETY: i < n <= dst.len()/2; escrita não alinhada permitida.
        unsafe { out.add(i).write_unaligned(u32_to_rgb565(p).to_be()) };
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn convert_avx2(src: &[u32], dst: &mut [u8]) {
    convert_chunked(src, dst)
}

fn convert_slice(src: &[u32], dst: &mut [u8]) {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            unsafe { convert_avx2(src, dst) };
            return;
        }
    }
    convert_chunked(src, dst);
}

/// Converte o quadro inteiro para RGB565 big-endian (meta: < 100 µs).
pub fn convert_frame(src: &[u32], dst: &mut [u8; FRAME_BYTES]) {
    debug_assert_eq!(src.len(), PIXELS);
    convert_slice(src, dst);
}

/// Orientação física do painel (corrige montagem invertida/espelhada).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Orientation {
    pub flip_x: bool,
    pub flip_y: bool,
}

/// Conversão + espelhamento em uma única passada, linha a linha.
pub fn convert_frame_oriented(src: &[u32], dst: &mut [u8; FRAME_BYTES], o: Orientation) {
    if o == Orientation::default() {
        return convert_frame(src, dst);
    }
    let mut row = [0u32; WIDTH];
    for y in 0..HEIGHT {
        let s = &src[y * WIDTH..(y + 1) * WIDTH];
        let dy = if o.flip_y { HEIGHT - 1 - y } else { y };
        let s: &[u32] = if o.flip_x {
            row.copy_from_slice(s);
            row.reverse();
            &row
        } else {
            s
        };
        convert_slice(s, &mut dst[dy * WIDTH * 2..(dy + 1) * WIDTH * 2]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };

    /// Aceita `#RGB`, `#RGBA`, `#RRGGBB` e `#RRGGBBAA`.
    pub fn parse(s: &str) -> Option<Color> {
        let h = s.trim().trim_start_matches('#');
        let hex = |i: usize, n: usize| u8::from_str_radix(&h[i..i + n], 16).ok();
        let dup = |v: u8| v * 17;
        Some(match h.len() {
            3 => Color { r: dup(hex(0, 1)?), g: dup(hex(1, 1)?), b: dup(hex(2, 1)?), a: 255 },
            4 => Color { r: dup(hex(0, 1)?), g: dup(hex(1, 1)?), b: dup(hex(2, 1)?), a: dup(hex(3, 1)?) },
            6 => Color { r: hex(0, 2)?, g: hex(2, 2)?, b: hex(4, 2)?, a: 255 },
            8 => Color { r: hex(0, 2)?, g: hex(2, 2)?, b: hex(4, 2)?, a: hex(6, 2)? },
            _ => return None,
        })
    }

    pub fn rgb(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    pub fn with_alpha_mul(self, f: f32) -> Color {
        Color { a: (self.a as f32 * f.clamp(0.0, 1.0)).round() as u8, ..self }
    }

    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }
    }

    /// Amostra um gradiente de N paradas igualmente espaçadas.
    pub fn gradient(stops: &[Color], t: f32) -> Color {
        match stops.len() {
            0 => Color::WHITE,
            1 => stops[0],
            n => {
                let t = t.clamp(0.0, 1.0) * (n - 1) as f32;
                let i = (t.floor() as usize).min(n - 2);
                Color::lerp(stops[i], stops[i + 1], t - i as f32)
            }
        }
    }
}

/// Operador "over" em ARGB pré-multiplicado (a em 0..=256).
/// Em um canvas opaco o alfa permanece 255; em uma camada transparente ele
/// acumula a cobertura, permitindo compor a camada depois sobre o fundo.
#[inline(always)]
fn blend(dst: u32, src_rgb: u32, a: u32) -> u32 {
    let src = src_rgb | 0xFF00_0000;
    let inv = 256 - a;
    let rb = (((src & 0xFF00FF) * a + (dst & 0xFF00FF) * inv) >> 8) & 0x00FF_00FF;
    let ag = (((src >> 8) & 0xFF00FF) * a + ((dst >> 8) & 0xFF00FF) * inv) & 0xFF00_FF00;
    rb | ag
}

pub struct Canvas {
    pub pixels: Vec<u32>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    pub const W: i32 = WIDTH as i32;
    pub const H: i32 = HEIGHT as i32;

    pub fn new() -> Self {
        Self { pixels: vec![0; PIXELS] }
    }

    pub fn clear(&mut self, rgb: u32) {
        self.pixels.fill(rgb);
    }

    /// Mistura um pixel com cobertura `cov` (0.0..=1.0) já multiplicada pela opacidade.
    #[inline(always)]
    pub fn blend_px(&mut self, x: i32, y: i32, c: Color, cov: f32) {
        if x < 0 || y < 0 || x >= Self::W || y >= Self::H {
            return;
        }
        let a = (cov * c.a as f32 * (256.0 / 255.0)) as u32;
        if a == 0 {
            return;
        }
        let i = (y * Self::W + x) as usize;
        self.pixels[i] = if a >= 256 { c.rgb() | 0xFF00_0000 } else { blend(self.pixels[i], c.rgb(), a) };
    }

    /// Compõe uma camada ARGB pré-multiplicada sobre este canvas.
    pub fn composite(&mut self, layer: &Canvas) {
        for (d, &o) in self.pixels.iter_mut().zip(layer.pixels.iter()) {
            let a = o >> 24;
            if a == 0 {
                continue;
            }
            if a == 255 {
                *d = o;
                continue;
            }
            let inv = 256 - (a + (a >> 7));
            let rb = (((*d & 0xFF00FF) * inv) >> 8) & 0xFF00FF;
            let g = (((*d & 0x00FF00) * inv) >> 8) & 0x00FF00;
            *d = (rb | g) + (o & 0x00FF_FFFF);
        }
    }

    /// Retângulo (x0, y0, x1, y1) exclusivo que contém todos os pixels com alfa > 0.
    pub fn alpha_bbox(&self) -> Option<(usize, usize, usize, usize)> {
        let (w, h) = (Self::W as usize, Self::H as usize);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            let row = &self.pixels[y * w..(y + 1) * w];
            if let Some(first) = row.iter().position(|p| p >> 24 != 0) {
                let last = row.iter().rposition(|p| p >> 24 != 0).unwrap();
                x0 = x0.min(first);
                x1 = x1.max(last + 1);
                y0 = y0.min(y);
                y1 = y + 1;
            }
        }
        (x1 > x0).then_some((x0, y0, x1, y1))
    }

    /// Compõe uma camada pré-multiplicada com opacidade global e deslocamento vertical.
    pub fn composite_alpha(&mut self, layer: &Canvas, alpha: f32, dy: i32) {
        if alpha >= 0.999 && dy == 0 {
            return self.composite(layer);
        }
        let k = (alpha.clamp(0.0, 1.0) * 256.0) as u32;
        let Some((bx0, by0, bx1, by1)) = layer.alpha_bbox() else { return };
        if k == 0 {
            return;
        }
        for sy in by0 as i32..by1 as i32 {
            let y = sy + dy;
            if !(0..Self::H).contains(&y) {
                continue;
            }
            let (drow, srow) = ((y * Self::W) as usize, (sy * Self::W) as usize);
            for x in bx0..bx1 {
                let o = layer.pixels[srow + x];
                if o >> 24 == 0 {
                    continue;
                }
                // Escala todos os canais (pré-multiplicados) pela opacidade.
                let rb = (((o & 0x00FF_00FF) * k) >> 8) & 0x00FF_00FF;
                let ag = ((((o >> 8) & 0x00FF_00FF) * k) >> 8) & 0x00FF_00FF;
                let a = ag >> 16;
                let src_rgb = rb | ((ag & 0xFF) << 8);
                let d = &mut self.pixels[drow + x];
                let inv = 256 - (a + (a >> 7));
                let drb = (((*d & 0xFF00FF) * inv) >> 8) & 0xFF00FF;
                let dg = (((*d & 0x00FF00) * inv) >> 8) & 0x00FF00;
                let da = ((((*d >> 24) * inv) >> 8) + a).min(255);
                *d = (da << 24) | ((drb | dg) + src_rgb);
            }
        }
    }

    /// Desenha um brilho difuso a partir do alfa de `layer` (box blur 3×).
    pub fn glow_from(&mut self, layer: &Canvas, c: Color, radius: f32, alpha: f32, dy: i32) {
        let r = radius.clamp(1.0, 24.0) as usize;
        let Some((bx0, by0, bx1, by1)) = layer.alpha_bbox() else { return };
        // Só a região ocupada + margem do desfoque (3 passadas ≈ 3r).
        let pad = 3 * r + 1;
        let (ox, oy) = (bx0.saturating_sub(pad), by0.saturating_sub(pad));
        let (ex, ey) = ((bx1 + pad).min(Self::W as usize), (by1 + pad).min(Self::H as usize));
        let (w, h) = (ex - ox, ey - oy);
        let mut a = vec![0u16; w * h];
        for y in 0..h {
            for x in 0..w {
                a[y * w + x] = (layer.pixels[(oy + y) * Self::W as usize + ox + x] >> 24) as u16;
            }
        }
        let mut tmp = vec![0u16; w * h];
        let blur = |src: &[u16], dst: &mut [u16], horizontal: bool| {
            let (n, m) = if horizontal { (h, w) } else { (w, h) };
            let idx = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
            let win = (2 * r + 1) as u32;
            for line in 0..n {
                let mut acc: u32 = 0;
                for i in 0..=r.min(m - 1) {
                    acc += src[idx(line, i)] as u32;
                }
                for i in 0..m {
                    dst[idx(line, i)] = (acc / win) as u16;
                    if i + r + 1 < m {
                        acc += src[idx(line, i + r + 1)] as u32;
                    }
                    if i >= r {
                        acc -= src[idx(line, i - r)] as u32;
                    }
                }
            }
        };
        for _ in 0..3 {
            blur(&a, &mut tmp, true);
            blur(&tmp, &mut a, false);
        }
        // Reforça o brilho (o blur dilui bastante) e aplica.
        let gain = 2.2 * alpha.clamp(0.0, 1.0);
        for y in 0..h {
            for x in 0..w {
                let v = a[y * w + x];
                if v > 0 {
                    self.blend_px((ox + x) as i32, (oy + y) as i32 + dy, c, (v as f32 / 255.0 * gain).min(1.0));
                }
            }
        }
    }

    /// Escurece/clareia o quadro inteiro (usado para dar contraste sobre vídeos).
    pub fn dim(&mut self, amount: f32) {
        let k = ((1.0 - amount.clamp(0.0, 1.0)) * 256.0) as u32;
        for p in &mut self.pixels {
            let rb = ((*p & 0xFF00FF) * k >> 8) & 0xFF00FF;
            let g = ((*p & 0x00FF00) * k >> 8) & 0x00FF00;
            *p = rb | g;
        }
    }

    /// Retângulo com cantos arredondados e anti-aliasing (SDF).
    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, c: Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
        let x0 = x.floor().max(0.0) as i32;
        let y0 = y.floor().max(0.0) as i32;
        let x1 = (x + w).ceil().min(Self::W as f32) as i32;
        let y1 = (y + h).ceil().min(Self::H as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let dx = ((px as f32 + 0.5 - cx).abs() - hx).max(0.0);
                let dy = ((py as f32 + 0.5 - cy).abs() - hy).max(0.0);
                let d = (dx * dx + dy * dy).sqrt() - r;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend_px(px, py, c, cov);
                }
            }
        }
    }

    /// Arco anti-aliased. Ângulos em graus, 0° = topo, sentido horário.
    /// `color_at(t)` recebe a posição normalizada 0..1 ao longo de `[a0, a1]`.
    #[allow(clippy::too_many_arguments)]
    pub fn arc<F: Fn(f32) -> Color>(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        width: f32,
        a0: f32,
        a1: f32,
        round_caps: bool,
        color_at: F,
    ) {
        if (a1 - a0).abs() < 0.01 || width <= 0.0 {
            return;
        }
        let (a0, a1) = if a1 < a0 { (a1, a0) } else { (a0, a1) };
        let span = a1 - a0;
        let r_in = radius - width / 2.0;
        let r_out = radius + width / 2.0;
        let half = width / 2.0;
        let cap = |a: f32| {
            let t = a.to_radians();
            (cx + radius * t.sin(), cy - radius * t.cos())
        };
        let (c0, c1) = (cap(a0), cap(a1));
        let x0 = (cx - r_out - 1.0).floor().max(0.0) as i32;
        let y0 = (cy - r_out - 1.0).floor().max(0.0) as i32;
        let x1 = (cx + r_out + 1.0).ceil().min(Self::W as f32) as i32;
        let y1 = (cy + r_out + 1.0).ceil().min(Self::H as f32) as i32;

        for py in y0..y1 {
            for px in x0..x1 {
                let fx = px as f32 + 0.5 - cx;
                let fy = py as f32 + 0.5 - cy;
                let d = (fx * fx + fy * fy).sqrt();
                if d < r_in - 1.0 || d > r_out + 1.0 {
                    continue;
                }
                // Ângulo do pixel no mesmo referencial (0 = topo, horário), normalizado perto de a0.
                let mut rel = (fx.atan2(-fy).to_degrees() - a0).rem_euclid(360.0);
                // Pixels logo antes do início ficam negativos (para o AA da borda).
                if rel > span + (360.0 - span) / 2.0 {
                    rel -= 360.0;
                }
                let ang = a0 + rel;
                let radial = (half - (d - radius).abs() + 0.5).clamp(0.0, 1.0);
                let ang_cov = if ang >= a0 && ang <= a1 {
                    // Suaviza a borda angular (~1px)
                    let px_per_deg = d * std::f32::consts::PI / 180.0;
                    (((ang - a0) * px_per_deg + 0.5).min((a1 - ang) * px_per_deg + 0.5)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let mut cov = radial * ang_cov;
                if round_caps {
                    for (ex, ey) in [c0, c1] {
                        let dx = px as f32 + 0.5 - ex;
                        let dy = py as f32 + 0.5 - ey;
                        let cd = (half - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
                        cov = cov.max(cd);
                    }
                }
                if cov > 0.0 {
                    let t = ((ang - a0) / span).clamp(0.0, 1.0);
                    self.blend_px(px, py, color_at(t), cov);
                }
            }
        }
    }

    /// Polilinha anti-aliased com espessura (distância ao segmento mais próximo).
    pub fn polyline(&mut self, pts: &[(f32, f32)], width: f32, c: Color) {
        if pts.len() < 2 {
            return;
        }
        let half = width / 2.0;
        let minx = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min) - half - 1.0;
        let maxx = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max) + half + 1.0;
        let miny = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min) - half - 1.0;
        let maxy = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) + half + 1.0;
        let x0 = minx.floor().max(0.0) as i32;
        let x1 = maxx.ceil().min(Self::W as f32) as i32;
        let y0 = miny.floor().max(0.0) as i32;
        let y1 = maxy.ceil().min(Self::H as f32) as i32;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            // Só os segmentos que cruzam esta coluna (pontos ordenados em x).
            let segs: Vec<_> = pts
                .windows(2)
                .filter(|s| s[0].0.min(s[1].0) - half - 1.0 <= fx && s[0].0.max(s[1].0) + half + 1.0 >= fx)
                .collect();
            for py in y0..y1 {
                let fy = py as f32 + 0.5;
                let mut best = f32::MAX;
                for s in &segs {
                    let (ax, ay) = s[0];
                    let (bx, by) = s[1];
                    let (vx, vy) = (bx - ax, by - ay);
                    let len2 = vx * vx + vy * vy;
                    let t = if len2 > 0.0 { (((fx - ax) * vx + (fy - ay) * vy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let (qx, qy) = (ax + vx * t - fx, ay + vy * t - fy);
                    best = best.min(qx * qx + qy * qy);
                }
                let cov = (half - best.sqrt() + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend_px(px, py, c, cov);
                }
            }
        }
    }

    /// Polilinha com pontos em qualquer ordem (formas livres/contornos).
    pub fn polyline_free(&mut self, pts: &[(f32, f32)], width: f32, c: Color) {
        if pts.len() < 2 {
            return;
        }
        let half = width / 2.0;
        let pad = half + 1.0;
        let x0 = (pts.iter().map(|p| p.0).fold(f32::MAX, f32::min) - pad).floor().max(0.0) as i32;
        let x1 = (pts.iter().map(|p| p.0).fold(f32::MIN, f32::max) + pad).ceil().min(Self::W as f32) as i32;
        let y0 = (pts.iter().map(|p| p.1).fold(f32::MAX, f32::min) - pad).floor().max(0.0) as i32;
        let y1 = (pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) + pad).ceil().min(Self::H as f32) as i32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let bw = (x1 - x0) as usize;
        let mut cov = vec![0f32; bw * (y1 - y0) as usize];
        for s in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (s[0], s[1]);
            let sx0 = (ax.min(bx) - pad).floor().max(x0 as f32) as i32;
            let sx1 = (ax.max(bx) + pad).ceil().min(x1 as f32) as i32;
            let sy0 = (ay.min(by) - pad).floor().max(y0 as f32) as i32;
            let sy1 = (ay.max(by) + pad).ceil().min(y1 as f32) as i32;
            let (vx, vy) = (bx - ax, by - ay);
            let len2 = vx * vx + vy * vy;
            for py in sy0..sy1 {
                for px in sx0..sx1 {
                    let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                    let t = if len2 > 0.0 { (((fx - ax) * vx + (fy - ay) * vy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let (qx, qy) = (ax + vx * t - fx, ay + vy * t - fy);
                    let k = (half - (qx * qx + qy * qy).sqrt() + 0.5).clamp(0.0, 1.0);
                    let i = (py - y0) as usize * bw + (px - x0) as usize;
                    cov[i] = cov[i].max(k);
                }
            }
        }
        for (i, &k) in cov.iter().enumerate() {
            if k > 0.0 {
                self.blend_px(x0 + (i % bw) as i32, y0 + (i / bw) as i32, c, k);
            }
        }
    }

    /// Preenche um polígono arbitrário (regra non-zero) com anti-aliasing:
    /// cobertura horizontal exata e 4 sub-linhas por pixel na vertical.
    /// `paint(x, y)` devolve cor e opacidade de cada pixel (degradês, níveis).
    pub fn fill_path<F: Fn(f32, f32) -> Option<(Color, f32)>>(&mut self, pts: &[(f32, f32)], paint: F) {
        if pts.len() < 3 {
            return;
        }
        let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(Self::W as f32) as i32;
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(Self::H as f32) as i32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let bw = (x1 - x0) as usize;
        let mut cov = vec![0f32; bw];
        let mut xs: Vec<(f32, i32)> = Vec::with_capacity(16);
        const SUB: usize = 4;
        for py in y0..y1 {
            cov.iter_mut().for_each(|c| *c = 0.0);
            for sub in 0..SUB {
                let sy = py as f32 + (sub as f32 + 0.5) / SUB as f32;
                xs.clear();
                for i in 0..pts.len() {
                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                    if (a.1 <= sy && b.1 > sy) || (b.1 <= sy && a.1 > sy) {
                        let x = a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0);
                        xs.push((x, if b.1 > a.1 { 1 } else { -1 }));
                    }
                }
                xs.sort_by(|p, q| p.0.total_cmp(&q.0));
                // Varre os cruzamentos acumulando o winding; preenche onde != 0.
                let mut wind = 0;
                for k in 0..xs.len() {
                    let before = wind;
                    wind += xs[k].1;
                    if before == 0 || k == 0 {
                        continue;
                    }
                    let (xa, xb) = (xs[k - 1].0.max(x0 as f32), xs[k].0.min(x1 as f32));
                    if xb <= xa {
                        continue;
                    }
                    let (pa, pb) = (xa.floor() as i32, (xb.ceil() as i32).min(x1));
                    for px in pa.max(x0)..pb {
                        let ov = (xb.min(px as f32 + 1.0) - xa.max(px as f32)).max(0.0);
                        cov[(px - x0) as usize] += ov / SUB as f32;
                    }
                }
            }
            for (i, &k) in cov.iter().enumerate() {
                if k <= 0.0 {
                    continue;
                }
                let px = x0 + i as i32;
                if let Some((col, op)) = paint(px as f32 + 0.5, py as f32 + 0.5) {
                    self.blend_px(px, py, col, k.min(1.0) * op);
                }
            }
        }
    }

    /// Área sob a curva com degradê vertical de transparência.
    pub fn fill_under(&mut self, pts: &[(f32, f32)], bottom: f32, c: Color) {
        if pts.len() < 2 {
            return;
        }
        let top = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let span = (bottom - top).max(1.0);
        for s in pts.windows(2) {
            let (ax, ay) = s[0];
            let (bx, by) = s[1];
            let xs = ax.floor() as i32;
            let xe = bx.ceil() as i32;
            for px in xs..xe {
                let fx = px as f32 + 0.5;
                if fx < ax || fx >= bx {
                    continue;
                }
                let t = if bx > ax { (fx - ax) / (bx - ax) } else { 0.0 };
                let yline = ay + (by - ay) * t;
                let ys = yline.ceil() as i32;
                // Borda superior parcial
                let frac = ys as f32 - yline;
                let fade_at = |y: f32| 1.0 - ((y - top) / span) * 0.85;
                if frac > 0.0 {
                    self.blend_px(px, ys - 1, c, frac * fade_at(yline));
                }
                for py in ys..bottom.ceil() as i32 {
                    self.blend_px(px, py, c, fade_at(py as f32));
                }
            }
        }
    }

    /// Máscara com recorte horizontal `[clip.0, clip.1)` e cor por linha (degradês).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_mask_ex<F: Fn(usize) -> Color>(&mut self, mask: &[u8], mw: usize, mh: usize, x: i32, y: i32, clip: (i32, i32), color_at: F) {
        for my in 0..mh {
            let py = y + my as i32;
            if py < 0 || py >= Self::H {
                continue;
            }
            let c = color_at(my);
            let row = &mask[my * mw..(my + 1) * mw];
            for (mx, &cov) in row.iter().enumerate() {
                let px = x + mx as i32;
                if cov != 0 && px >= clip.0 && px < clip.1 {
                    self.blend_px(px, py, c, cov as f32 / 255.0);
                }
            }
        }
    }

    /// Aplica uma máscara de cobertura (texto) na posição dada.
    pub fn draw_mask(&mut self, mask: &[u8], mw: usize, mh: usize, x: i32, y: i32, c: Color) {
        for my in 0..mh {
            let py = y + my as i32;
            if py < 0 || py >= Self::H {
                continue;
            }
            let row = &mask[my * mw..(my + 1) * mw];
            for (mx, &cov) in row.iter().enumerate() {
                if cov != 0 {
                    self.blend_px(x + mx as i32, py, c, cov as f32 / 255.0);
                }
            }
        }
    }

    /// Desenha uma imagem RGBA (não pré-multiplicada) com opacidade global.
    pub fn blit_rgba(&mut self, img: &[[u8; 4]], iw: usize, ih: usize, x: i32, y: i32, opacity: f32) {
        for iy in 0..ih {
            for ix in 0..iw {
                let p = img[iy * iw + ix];
                if p[3] == 0 {
                    continue;
                }
                let c = Color { r: p[0], g: p[1], b: p[2], a: p[3] };
                self.blend_px(x + ix as i32, y + iy as i32, c, opacity);
            }
        }
    }

    pub fn to_rgb565_be(&self, out: &mut [u8; FRAME_BYTES]) {
        convert_frame(&self.pixels, out);
    }

    pub fn to_rgb565_be_oriented(&self, out: &mut [u8; FRAME_BYTES], o: Orientation) {
        convert_frame_oriented(&self.pixels, out, o);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_and_vector_match() {
        let src: Vec<u32> = (0..PIXELS as u32).map(|i| i.wrapping_mul(2654435761) & 0xFFFFFF).collect();
        let mut out = Box::new([0u8; FRAME_BYTES]);
        convert_frame(&src, &mut out);
        for (i, p) in src.iter().enumerate() {
            let (hi, lo) = rgb888_to_rgb565_be((p >> 16) as u8, (p >> 8) as u8, *p as u8);
            assert_eq!((out[2 * i], out[2 * i + 1]), (hi, lo), "pixel {i}");
        }
    }

    #[test]
    fn conversion_speed() {
        let src: Vec<u32> = (0..PIXELS as u32).collect();
        let mut out = Box::new([0u8; FRAME_BYTES]);
        // Melhor de 5 lotes: imune a picos de carga da máquina.
        let per = (0..5)
            .map(|_| {
                let n = 100;
                let t = std::time::Instant::now();
                for _ in 0..n {
                    convert_frame(std::hint::black_box(&src), &mut out);
                }
                t.elapsed() / n
            })
            .min()
            .unwrap();
        println!("convert_frame: {per:?} por quadro");
        if !cfg!(debug_assertions) {
            assert!(per.as_micros() < 100);
        }
    }

    #[test]
    fn overlay_matches_direct_drawing() {
        let col = Color { r: 255, g: 40, b: 200, a: 160 };
        let mut direct = Canvas::new();
        direct.clear(0x203040);
        direct.fill_round_rect(10.3, 10.7, 100.0, 50.0, 12.0, col);
        direct.fill_round_rect(40.0, 30.0, 100.0, 50.0, 6.0, Color { a: 90, ..Color::WHITE });

        let mut layer = Canvas::new();
        layer.fill_round_rect(10.3, 10.7, 100.0, 50.0, 12.0, col);
        layer.fill_round_rect(40.0, 30.0, 100.0, 50.0, 6.0, Color { a: 90, ..Color::WHITE });
        let mut composed = Canvas::new();
        composed.clear(0x203040);
        composed.composite(&layer);

        for (a, b) in direct.pixels.iter().zip(&composed.pixels) {
            for sh in [0, 8, 16] {
                let (x, y) = ((a >> sh) & 0xFF, (b >> sh) & 0xFF);
                assert!(x.abs_diff(y) <= 3, "{a:08X} vs {b:08X}");
            }
        }
    }

    #[test]
    fn roundtrip_565() {
        assert_eq!(rgb565_to_u32(u32_to_rgb565(0xFFFFFF)), 0xFFFFFF);
        assert_eq!(rgb565_to_u32(u32_to_rgb565(0)), 0);
        assert_eq!(Color::parse("#FF000080"), Some(Color { r: 255, g: 0, b: 0, a: 128 }));
    }
}
