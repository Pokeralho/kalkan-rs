//! Fundos de tela: cor sólida, imagem estática, animações (GIF/WebP/APNG) e vídeo (MP4/H.264).
//!
//! Todos os fundos entregam pixels já ajustados a 320x240 para o compositor.

pub mod gif;
pub mod video;

use crate::driver::{HEIGHT, PIXELS, WIDTH};
use crate::renderer::canvas::{rgb565_to_u32, Canvas};
use image::RgbaImage;
use std::path::Path;
use std::time::Instant;

/// Como a imagem/vídeo é ajustada à tela 320x240.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Preenche a tela recortando o excesso (padrão).
    #[default]
    Cover,
    /// Mostra a imagem inteira com tarjas.
    Contain,
    /// Estica sem manter proporção.
    Stretch,
}

/// Calcula o tamanho intermediário (antes do recorte central) para `Cover`.
pub fn cover_size(w: u32, h: u32) -> (u32, u32) {
    let s = (WIDTH as f64 / w as f64).max(HEIGHT as f64 / h as f64);
    let tw = ((w as f64 * s).ceil() as u32).max(WIDTH as u32);
    let th = ((h as f64 * s).ceil() as u32).max(HEIGHT as u32);
    (tw, th)
}

/// Redimensiona uma imagem RGBA para 320x240 (0x00RRGGBB), compondo sobre preto.
pub fn fit_image(img: &RgbaImage, fit: Fit) -> Vec<u32> {
    use image::imageops::{resize, FilterType};
    let (w, h) = img.dimensions();
    let mut out = vec![0u32; PIXELS];
    if w == 0 || h == 0 {
        return out;
    }
    let filter = FilterType::Triangle;
    let (scaled, ox, oy, sx, sy) = match fit {
        Fit::Stretch => (resize(img, WIDTH as u32, HEIGHT as u32, filter), 0i64, 0i64, 0u32, 0u32),
        Fit::Cover => {
            let (tw, th) = cover_size(w, h);
            let r = if (tw, th) == (w, h) { img.clone() } else { resize(img, tw, th, filter) };
            (r, 0, 0, (tw - WIDTH as u32) / 2, (th - HEIGHT as u32) / 2)
        }
        Fit::Contain => {
            let s = (WIDTH as f64 / w as f64).min(HEIGHT as f64 / h as f64);
            let tw = ((w as f64 * s).round() as u32).clamp(1, WIDTH as u32);
            let th = ((h as f64 * s).round() as u32).clamp(1, HEIGHT as u32);
            let r = resize(img, tw, th, filter);
            (r, (WIDTH as i64 - tw as i64) / 2, (HEIGHT as i64 - th as i64) / 2, 0, 0)
        }
    };
    let (sw, sh) = scaled.dimensions();
    for y in 0..HEIGHT as i64 {
        let syy = y - oy + sy as i64;
        if syy < 0 || syy >= sh as i64 {
            continue;
        }
        for x in 0..WIDTH as i64 {
            let sxx = x - ox + sx as i64;
            if sxx < 0 || sxx >= sw as i64 {
                continue;
            }
            let p = scaled.get_pixel(sxx as u32, syy as u32).0;
            let a = p[3] as u32;
            let r = p[0] as u32 * a / 255;
            let g = p[1] as u32 * a / 255;
            let b = p[2] as u32 * a / 255;
            out[(y as usize) * WIDTH + x as usize] = (r << 16) | (g << 8) | b;
        }
    }
    out
}

/// Desfoque de caixa (3 passadas ≈ gaussiano) sobre pixels 0x00RRGGBB.
pub fn blur_rgb(px: &mut [u32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    let mut tmp = vec![0u32; px.len()];
    let pass = |src: &[u32], dst: &mut [u32], horizontal: bool| {
        let (n, m) = if horizontal { (h, w) } else { (w, h) };
        let idx = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
        let r = radius.min(m - 1);
        let win = (2 * r + 1) as u32;
        for line in 0..n {
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            // Borda estendida (clamp) evita escurecer as laterais.
            let at = |i: isize| src[idx(line, i.clamp(0, m as isize - 1) as usize)];
            for i in -(r as isize)..=(r as isize) {
                let p = at(i);
                sr += (p >> 16) & 0xFF;
                sg += (p >> 8) & 0xFF;
                sb += p & 0xFF;
            }
            for i in 0..m {
                dst[idx(line, i)] = ((sr / win) << 16) | ((sg / win) << 8) | (sb / win);
                let (add, sub) = (at(i as isize + r as isize + 1), at(i as isize - r as isize));
                sr = sr + ((add >> 16) & 0xFF) - ((sub >> 16) & 0xFF);
                sg = sg + ((add >> 8) & 0xFF) - ((sub >> 8) & 0xFF);
                sb = sb + (add & 0xFF) - (sub & 0xFF);
            }
        }
    };
    for _ in 0..3 {
        pass(px, &mut tmp, true);
        pass(&tmp, px, false);
    }
}

/// Fundo gerado a partir da capa da música (desfocada), com crossfade na troca.
pub struct AlbumBg {
    id: Option<u64>,
    cur: Vec<u32>,
    prev: Vec<u32>,
    fade_start: Instant,
    blur: usize,
    fallback: u32,
}

impl AlbumBg {
    pub fn new(fallback: u32, blur: f32) -> Self {
        Self {
            id: None,
            cur: vec![fallback; PIXELS],
            prev: vec![fallback; PIXELS],
            fade_start: Instant::now() - std::time::Duration::from_secs(10),
            blur: if blur > 0.0 { blur as usize } else { 14 },
            fallback,
        }
    }

    /// Atualiza para a capa atual; retorna `true` se mudou.
    pub fn sync(&mut self, art: Option<&crate::telemetry::media::AlbumArt>) -> bool {
        let id = art.map(|a| a.id);
        if id == self.id {
            return false;
        }
        self.id = id;
        let next = match art {
            Some(a) => {
                let mut px = fit_image(&a.img, Fit::Cover);
                blur_rgb(&mut px, WIDTH, HEIGHT, self.blur);
                px
            }
            None => vec![self.fallback; PIXELS],
        };
        self.prev = std::mem::replace(&mut self.cur, next);
        self.fade_start = Instant::now();
        true
    }

    fn fade(&self) -> f32 {
        (self.fade_start.elapsed().as_secs_f32() / 0.8).clamp(0.0, 1.0)
    }
}

pub enum Background {
    Solid(u32),
    Album(AlbumBg),
    Static(Vec<u32>),
    Animated(gif::AnimPlayer),
    Video(video::VideoPlayer),
}

impl Background {
    pub fn load(kind: &str, path: Option<&Path>, color: u32, fit: Fit, cache_bytes: usize, blur: f32) -> anyhow::Result<Self> {
        let ext = || {
            path.and_then(|p| p.extension())
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        };
        let kind = match kind {
            "" | "auto" => match ext().as_str() {
                "mp4" | "m4v" | "mov" | "wmv" | "avi" | "mkv" => "video",
                "gif" | "webp" | "apng" => "gif",
                "png" if gif::is_apng(path.unwrap()) => "gif",
                _ if path.is_some() => "static",
                _ => "solid",
            },
            k => k,
        };
        Ok(match kind {
            "solid" | "color" => Background::Solid(color),
            "album" | "cover" => Background::Album(AlbumBg::new(color, blur)),
            "static" | "image" => {
                let p = path.ok_or_else(|| anyhow::anyhow!("background estático sem 'path'"))?;
                let img = image::open(p)?.to_rgba8();
                let mut px = fit_image(&img, fit);
                blur_rgb(&mut px, WIDTH, HEIGHT, blur.max(0.0) as usize);
                Background::Static(px)
            }
            "gif" | "animation" | "webp" | "apng" => {
                let p = path.ok_or_else(|| anyhow::anyhow!("background animado sem 'path'"))?;
                Background::Animated(gif::AnimPlayer::open(p, fit, cache_bytes)?)
            }
            "video" | "mp4" => {
                let p = path.ok_or_else(|| anyhow::anyhow!("background de vídeo sem 'path'"))?;
                Background::Video(video::VideoPlayer::open(p)?)
            }
            other => anyhow::bail!("tipo de background desconhecido: {other}"),
        })
    }

    /// Avança a animação; retorna `true` se o frame mudou.
    pub fn advance(&mut self, now: Instant) -> bool {
        match self {
            Background::Animated(a) => a.advance(now),
            Background::Album(a) => a.fade() < 1.0,
            Background::Video(v) => v.advance(now),
            _ => false,
        }
    }

    /// Próximo instante em que o fundo precisa ser redesenhado.
    pub fn next_deadline(&self) -> Option<Instant> {
        match self {
            Background::Animated(a) => a.next_deadline(),
            Background::Video(v) => v.next_deadline(),
            Background::Album(a) if a.fade() < 1.0 => Some(Instant::now() + std::time::Duration::from_millis(33)),
            _ => None,
        }
    }

    pub fn draw(&self, c: &mut Canvas) {
        match self {
            Background::Solid(col) => c.clear(*col),
            Background::Album(a) => {
                let k = a.fade();
                if k >= 1.0 {
                    c.pixels.copy_from_slice(&a.cur);
                } else {
                    let w = (k * 256.0) as u32;
                    for ((d, &n), &o) in c.pixels.iter_mut().zip(&a.cur).zip(&a.prev) {
                        let rb = (((n & 0xFF00FF) * w + (o & 0xFF00FF) * (256 - w)) >> 8) & 0xFF00FF;
                        let g = (((n & 0x00FF00) * w + (o & 0x00FF00) * (256 - w)) >> 8) & 0x00FF00;
                        *d = rb | g;
                    }
                }
            }
            Background::Static(px) => c.pixels.copy_from_slice(px),
            Background::Animated(a) => match a.current() {
                Some(f) => {
                    for (d, s) in c.pixels.iter_mut().zip(f.iter()) {
                        *d = rgb565_to_u32(*s);
                    }
                }
                None => c.clear(0),
            },
            Background::Video(v) => match v.current() {
                Some(f) => c.pixels.copy_from_slice(f),
                None => c.clear(0),
            },
        }
    }
}
