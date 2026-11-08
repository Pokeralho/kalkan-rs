//! Camadas do tema: texto, gauges, barras, sparklines, imagens, retângulos,
//! formas livres e grade de núcleos. Cada camada é definida no `theme.toml`
//! (structs `*Def`) e ganha comportamentos comuns (condições, regras de cor,
//! animações, brilho e opacidade) via [`Behavior`].

use super::canvas::{Canvas, Color};
use super::expr::Expr;
use crate::telemetry::Snapshot;
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ───────────────────────────── Fontes ─────────────────────────────

pub struct FontStore {
    search: Vec<PathBuf>,
    loaded: HashMap<String, FontArc>,
    fallbacks: Vec<FontArc>,
}

impl FontStore {
    pub fn new(theme_dir: &Path, fonts_dir: &Path) -> Self {
        let mut search = vec![theme_dir.to_path_buf(), theme_dir.join("fonts"), fonts_dir.to_path_buf()];
        if let Some(win) = std::env::var_os("WINDIR") {
            search.push(PathBuf::from(win).join("Fonts"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            search.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
        let mut s = Self { search, loaded: HashMap::new(), fallbacks: Vec::new() };
        // Cadeia de fallback para glifos ausentes (símbolos ▶ ° etc).
        for f in ["segoeui.ttf", "seguisym.ttf", "arial.ttf"] {
            if let Some(font) = s.load(f) {
                s.fallbacks.push(font);
            }
        }
        s
    }

    fn load(&mut self, name: &str) -> Option<FontArc> {
        if let Some(f) = self.loaded.get(name) {
            return Some(f.clone());
        }
        let candidates: Vec<PathBuf> = if Path::new(name).is_absolute() {
            vec![PathBuf::from(name)]
        } else {
            self.search.iter().map(|d| d.join(name)).collect()
        };
        for p in candidates {
            if let Ok(bytes) = std::fs::read(&p) {
                match FontArc::try_from_vec(bytes) {
                    Ok(f) => {
                        self.loaded.insert(name.to_string(), f.clone());
                        return Some(f);
                    }
                    Err(e) => log::warn!("fonte inválida {}: {e}", p.display()),
                }
            }
        }
        None
    }

    /// Fonte pelo nome do arquivo; cai para a primeira fonte de fallback.
    pub fn get(&mut self, name: Option<&str>) -> Option<FontArc> {
        name.and_then(|n| {
            let f = self.load(n);
            if f.is_none() {
                log::warn!("fonte '{n}' não encontrada; usando fallback");
            }
            f
        })
        .or_else(|| self.fallbacks.first().cloned())
    }

    pub fn fallbacks(&self) -> &[FontArc] {
        &self.fallbacks
    }
}

/// Máscara de cobertura (alpha) de uma linha de texto.
#[derive(Clone, Default)]
pub struct TextMask {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

fn px_scale(font: &FontArc, size: f32) -> PxScale {
    // `size` em pixels por em (como CSS), não altura de linha.
    let upem = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(size * font.height_unscaled() / upem)
}

fn pick_font<'a>(c: char, primary: &'a FontArc, fallbacks: &'a [FontArc]) -> &'a FontArc {
    if primary.glyph_id(c).0 != 0 {
        return primary;
    }
    fallbacks.iter().find(|f| f.glyph_id(c).0 != 0).unwrap_or(primary)
}

fn measure(text: &str, primary: &FontArc, fallbacks: &[FontArc], size: f32, spacing: f32) -> f32 {
    let mut x = 0.0;
    let mut prev: Option<(ab_glyph::GlyphId, *const FontArc)> = None;
    for c in text.chars() {
        let f = pick_font(c, primary, fallbacks);
        let sf = f.as_scaled(px_scale(f, size));
        let id = f.glyph_id(c);
        if let Some((pid, pf)) = prev {
            if std::ptr::eq(pf, f) {
                x += sf.kern(pid, id);
            }
        }
        x += sf.h_advance(id) + spacing;
        prev = Some((id, f as *const _));
    }
    x - if text.is_empty() { 0.0 } else { spacing }
}

pub fn render_text(text: &str, primary: &FontArc, fallbacks: &[FontArc], size: f32, spacing: f32) -> TextMask {
    let sp = primary.as_scaled(px_scale(primary, size));
    let ascent = sp.ascent();
    let h = (ascent - sp.descent()).ceil().max(1.0) as usize;
    let w = (measure(text, primary, fallbacks, size, spacing).ceil().max(0.0) as usize) + 2;
    let mut data = vec![0u8; w * h];

    let mut x = 0.0f32;
    let mut prev: Option<(ab_glyph::GlyphId, *const FontArc)> = None;
    for c in text.chars() {
        let f = pick_font(c, primary, fallbacks);
        let sf = f.as_scaled(px_scale(f, size));
        let id = f.glyph_id(c);
        if let Some((pid, pf)) = prev {
            if std::ptr::eq(pf, f) {
                x += sf.kern(pid, id);
            }
        }
        let glyph = id.with_scale_and_position(sf.scale(), point(x, ascent));
        if let Some(og) = f.outline_glyph(glyph) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cov| {
                let px = b.min.x as i32 + gx as i32;
                let py = b.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    let i = py as usize * w + px as usize;
                    data[i] = data[i].max((cov * 255.0).round().min(255.0) as u8);
                }
            });
        }
        x += sf.h_advance(id) + spacing;
        prev = Some((id, f as *const _));
    }
    TextMask { w, h, data }
}

// ───────────────────────── Ambiente de avaliação ─────────────────────────

/// Variáveis disponíveis para expressões: métricas da telemetria, `t`
/// (segundos desde o carregamento do tema) e campos do relógio.
pub struct Env<'a> {
    pub snap: &'a Snapshot,
    pub now: DateTime<Local>,
    pub t: f32,
}

impl Env<'_> {
    pub fn get(&self, k: &str) -> Option<f64> {
        use chrono::{Datelike, Timelike};
        Some(match k {
            "t" => self.t as f64,
            "hour" => self.now.hour() as f64,
            "minute" => self.now.minute() as f64,
            "second" => self.now.second() as f64,
            "day" => self.now.day() as f64,
            "month" => self.now.month() as f64,
            "year" => self.now.year() as f64,
            "weekday_num" => self.now.weekday().number_from_monday() as f64,
            _ => return self.snap.metric(k),
        })
    }

    pub fn eval(&self, e: &Expr) -> Option<f64> {
        e.eval(&|k| self.get(k))
    }
}

fn parse_expr(src: &str, what: &str) -> Option<Expr> {
    match Expr::parse(src) {
        Ok(e) => Some(e),
        Err(err) => {
            log::warn!("{what} '{src}': {err}");
            None
        }
    }
}

// ───────────────────────── Templates de texto ─────────────────────────

#[derive(Debug, Clone)]
enum Seg {
    Lit(String),
    /// Expressão numérica com largura mínima e casas decimais.
    Num { expr: Expr, width: usize, prec: usize },
    /// Valor textual (nomes, mídia, taxas formatadas).
    Text(String),
    Time(String),
}

/// Texto com placeholders: `{cpu_temp}`, `{cpu_temp:.1}`, `{cpu_temp*1.8+32:.0}`,
/// `{time:%H:%M:%S}`, `{media_title}`.
#[derive(Debug, Clone)]
pub struct Template {
    segs: Vec<Seg>,
    pub uses_time: bool,
    pub uses_seconds: bool,
}

pub const TEXT_KEYS: &[&str] = &[
    "cpu_name", "gpu_name", "media_title", "media_artist", "media_album", "media_state", "media_icon",
    "net_down", "net_up", "disk_read", "disk_write", "weather_desc",
];

impl Template {
    pub fn parse(s: &str) -> Self {
        let mut segs = Vec::new();
        let mut lit = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    lit.push('{');
                    continue;
                }
                let mut inner = String::new();
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        closed = true;
                        break;
                    }
                    inner.push(c2);
                }
                if !closed {
                    lit.push('{');
                    lit.push_str(&inner);
                    continue;
                }
                if !lit.is_empty() {
                    segs.push(Seg::Lit(std::mem::take(&mut lit)));
                }
                // O formato fica após o ÚLTIMO ':' que não faz parte de strftime.
                let (key, spec) = match inner.split_once(':') {
                    Some((k, s)) => (k.trim().to_string(), Some(s.to_string())),
                    None => (inner.trim().to_string(), None),
                };
                segs.push(match key.as_str() {
                    "time" => Seg::Time(spec.unwrap_or_else(|| "%H:%M".into())),
                    "date" => Seg::Time(spec.unwrap_or_else(|| "%d/%m/%Y".into())),
                    "weekday" => Seg::Time("%A".into()),
                    k if TEXT_KEYS.contains(&k) => Seg::Text(key),
                    _ => {
                        let (mut width, mut prec) = (0, 0);
                        if let Some(sp) = spec {
                            let (w, p) = sp.split_once('.').unwrap_or((&sp, ""));
                            width = w.trim_start_matches('>').parse().unwrap_or(0);
                            prec = p.parse().unwrap_or(0);
                        }
                        match parse_expr(&key, "placeholder") {
                            Some(expr) => Seg::Num { expr, width, prec },
                            None => Seg::Lit(format!("{{{inner}}}")),
                        }
                    }
                });
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                lit.push('}');
            } else {
                lit.push(c);
            }
        }
        if !lit.is_empty() {
            segs.push(Seg::Lit(lit));
        }
        let uses_time = segs.iter().any(|s| matches!(s, Seg::Time(_)));
        let uses_seconds = segs.iter().any(|s| matches!(s, Seg::Time(f) if f.contains("%S") || f.contains("%T") || f.contains("%X")));
        Self { segs, uses_time, uses_seconds }
    }

    pub fn render(&self, env: &Env) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        for s in &self.segs {
            match s {
                Seg::Lit(l) => out.push_str(l),
                Seg::Text(k) => out.push_str(&env.snap.text(k).unwrap_or_else(|| "--".into())),
                Seg::Time(f) => {
                    let _ = write!(out, "{}", env.now.format(f));
                }
                Seg::Num { expr, width, prec } => match env.eval(expr) {
                    Some(v) => {
                        let _ = write!(out, "{v:>width$.prec$}", width = *width, prec = *prec);
                    }
                    None => out.push_str("--"),
                },
            }
        }
        out
    }
}

// ─────────────────────────── Definições TOML ───────────────────────────

fn d_one() -> f32 { 1.0 }
fn d_true() -> bool { true }
fn d_white() -> String { "#FFFFFF".into() }
fn d_track() -> String { "#FFFFFF22".into() }
fn d_100() -> f64 { 100.0 }
fn d_size() -> f32 { 16.0 }
fn d_stroke() -> f32 { 8.0 }
fn d_spark_stroke() -> f32 { 1.5 }
fn d_seconds() -> usize { 30 }
fn d_fill_opacity() -> f32 { 0.25 }
fn d_gap() -> f32 { 2.0 }
fn is_one(v: &f32) -> bool { (*v - 1.0).abs() < 1e-6 }
fn is_zero(v: &f32) -> bool { *v == 0.0 }
fn is_false(v: &bool) -> bool { !*v }
fn is_one_u(v: &u32) -> bool { *v == 1 }
fn one_u() -> u32 { 1 }

/// Regra de cor condicional: a primeira regra verdadeira substitui a cor
/// principal da camada.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ColorRule {
    pub when: String,
    pub color: String,
}

/// Comportamentos disponíveis em TODAS as camadas.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Behavior {
    /// Expressão: a camada só aparece quando for verdadeira.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_if: Option<String>,
    /// Opacidade da camada inteira (0..1).
    #[serde(skip_serializing_if = "is_one")]
    pub layer_opacity: f32,
    /// `pulse`, `blink`, `spin`, `marquee`, `fade_in`, `slide_up`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
    #[serde(skip_serializing_if = "is_one")]
    pub animation_speed: f32,
    /// Cor do brilho (glow) ao redor da camada.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glow: Option<String>,
    #[serde(skip_serializing_if = "is_zero")]
    pub glow_radius: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub color_rules: Vec<ColorRule>,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            visible_if: None,
            layer_opacity: 1.0,
            animation: None,
            animation_speed: 1.0,
            glow: None,
            glow_radius: 0.0,
            color_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TextDef {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default = "d_size")]
    pub size: f32,
    pub x: f32,
    pub y: f32,
    #[serde(default = "d_white", alias = "color")]
    pub color_hex: String,
    #[serde(default)]
    pub align: String,
    /// `top` (padrão), `middle` ou `bottom` – referência vertical de `y`.
    #[serde(default)]
    pub valign: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<String>,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default = "d_one")]
    pub opacity: f32,
    /// Contorno do texto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outline_width: f32,
    /// Degradê vertical do preenchimento (sobrepõe `color_hex`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gradient: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub uppercase: bool,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GaugeDef {
    /// Métrica ou expressão (ex.: `cpu_temp`, `gpu_usage * 2`).
    pub metric: String,
    #[serde(default)]
    pub min: f64,
    #[serde(default = "d_100")]
    pub max: f64,
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
    #[serde(default = "default_start")]
    pub start_angle: f32,
    #[serde(default = "default_end")]
    pub end_angle: f32,
    #[serde(default = "d_stroke")]
    pub stroke_width: f32,
    #[serde(default)]
    pub color_gradient: Vec<String>,
    #[serde(default = "d_track")]
    pub track_color: String,
    /// `arc` = degradê ao longo do arco; `value` = cor única pelo valor atual.
    #[serde(default)]
    pub color_mode: String,
    #[serde(default = "d_true")]
    pub rounded_caps: bool,
    #[serde(default = "d_true")]
    pub smooth: bool,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}
fn default_start() -> f32 { -135.0 }
fn default_end() -> f32 { 135.0 }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BarDef {
    pub metric: String,
    #[serde(default)]
    pub min: f64,
    #[serde(default = "d_100")]
    pub max: f64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub orientation: String,
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub color_gradient: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_hex: Option<String>,
    #[serde(default = "d_track")]
    pub track_color: String,
    #[serde(default = "d_true")]
    pub smooth: bool,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SparklineDef {
    /// Métrica com histórico (ver `HISTORY_KEYS`).
    pub metric: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default = "d_seconds")]
    pub seconds: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default = "d_white", alias = "color")]
    pub color_hex: String,
    #[serde(default = "d_fill_opacity")]
    pub fill_opacity: f32,
    #[serde(default = "d_spark_stroke")]
    pub stroke_width: f32,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageDef {
    pub path: String,
    pub x: f32,
    pub y: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default = "d_one")]
    pub opacity: f32,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RectDef {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub radius: f32,
    #[serde(alias = "color", default = "d_track")]
    pub color_hex: String,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

/// Grade de barras com o uso de cada núcleo lógico da CPU.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoresDef {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Núcleos por linha (0 = todos numa linha só).
    #[serde(default)]
    pub columns: u32,
    #[serde(default = "d_gap")]
    pub gap: f32,
    /// `vertical` (padrão) ou `horizontal`.
    #[serde(default)]
    pub orientation: String,
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub color_gradient: Vec<String>,
    #[serde(default = "d_track")]
    pub track_color: String,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

fn d_shape_kind() -> String { "rect".into() }
fn d_sides() -> u32 { 5 }
fn d_inner() -> f32 { 0.5 }
fn d_fill() -> String { "#FFFFFF".into() }
fn d_fill_dir() -> f32 { 270.0 }

/// Forma livre: polígono arbitrário, estrela, polígono regular, elipse,
/// retângulo arredondado ou linha. Coordenadas de `points`/`paths` são
/// normalizadas (0..1) dentro da caixa `x/y/width/height`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShapeDef {
    /// `rect`, `ellipse`, `polygon`, `regular`, `star`, `line`
    #[serde(default = "d_shape_kind")]
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// Traços extras (apenas contorno), cada um uma lista de pontos.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Vec<[f32; 2]>>,
    #[serde(default = "d_sides")]
    pub sides: u32,
    #[serde(default = "d_inner")]
    pub inner_ratio: f32,
    #[serde(default)]
    pub corner_radius: f32,
    /// Rotação em graus em torno do centro.
    #[serde(default)]
    pub rotation: f32,
    /// Repetição radial: desenha N cópias giradas em torno do centro.
    #[serde(default = "one_u", skip_serializing_if = "is_one_u")]
    pub repeat: u32,
    #[serde(default = "d_fill")]
    pub fill: String,
    #[serde(default)]
    pub fill_gradient: Vec<String>,
    /// `linear` ou `radial`
    #[serde(default)]
    pub gradient_type: String,
    #[serde(default)]
    pub gradient_angle: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default)]
    pub stroke_width: f32,
    #[serde(default = "d_one")]
    pub opacity: f32,
    /// Métrica/expressão opcional: preenche a forma proporcionalmente.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(default)]
    pub min: f64,
    #[serde(default = "d_100")]
    pub max: f64,
    /// Direção do preenchimento pela métrica (270 = de baixo para cima).
    #[serde(default = "d_fill_dir")]
    pub fill_direction: f32,
    #[serde(default)]
    pub z: i32,
    #[serde(flatten)]
    pub behavior: Behavior,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct WidgetDefs {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<TextDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub radial_gauge: Vec<GaugeDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bar: Vec<BarDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sparkline: Vec<SparklineDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub image: Vec<ImageDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rect: Vec<RectDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shape: Vec<ShapeDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cores: Vec<CoresDef>,
}

// ───────────────────────────── Cores e paleta ─────────────────────────────

/// Paleta do tema: cores nomeadas usadas como `$nome` em qualquer campo de cor.
pub type Palette = HashMap<String, Color>;

thread_local! {
    static PALETTE: std::cell::RefCell<Palette> = std::cell::RefCell::new(HashMap::new());
}

pub fn color(s: &str) -> Color {
    let s = s.trim();
    if let Some(name) = s.strip_prefix('$') {
        if let Some(c) = PALETTE.with(|p| p.borrow().get(name).copied()) {
            return c;
        }
        log::warn!("cor '${name}' não existe na paleta");
        return Color::WHITE;
    }
    Color::parse(s).unwrap_or_else(|| {
        log::warn!("cor inválida '{s}'");
        Color::WHITE
    })
}

fn colors(v: &[String], fallback: Color) -> Vec<Color> {
    if v.is_empty() {
        vec![fallback]
    } else {
        v.iter().map(|s| color(s)).collect()
    }
}

// ─────────────────────────── Runtime ───────────────────────────

/// Valor exibido com suavização exponencial (animação entre amostras de 1 Hz).
#[derive(Default)]
struct Smoothed {
    shown: Option<f32>,
}

impl Smoothed {
    /// Retorna (valor exibido, ainda animando?)
    fn step(&mut self, target: f32, dt: f32, smooth: bool) -> (f32, bool) {
        let cur = match self.shown {
            Some(c) if smooth => c,
            _ => {
                self.shown = Some(target);
                return (target, false);
            }
        };
        let k = 1.0 - (-dt * 12.0).exp();
        let mut next = cur + (target - cur) * k;
        let animating = (target - next).abs() > 0.002;
        if !animating {
            next = target;
        }
        self.shown = Some(next);
        (next, animating)
    }
}

fn norm(v: Option<f64>, min: f64, max: f64) -> f32 {
    let v = v.unwrap_or(min);
    if max > min { ((v - min) / (max - min)).clamp(0.0, 1.0) as f32 } else { 0.0 }
}

/// Contexto de um quadro.
pub struct FrameCtx<'a> {
    pub env: Env<'a>,
    pub dt: f32,
    pub fallbacks: &'a [FontArc],
}

/// Ajustes aplicados pelos comportamentos ao conteúdo da camada.
#[derive(Default, Clone, Copy)]
struct Overrides {
    color: Option<Color>,
    rotation: f32,
    /// Deslocamento do letreiro (px) para textos com `marquee`.
    marquee: Option<f32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Anim {
    None,
    Pulse,
    Blink,
    Spin,
    Marquee,
    FadeIn,
    SlideUp,
}

struct BehaviorRt {
    visible: Option<Expr>,
    opacity: f32,
    anim: Anim,
    speed: f32,
    glow: Option<(Color, f32)>,
    rules: Vec<(Expr, Color)>,
}

impl BehaviorRt {
    fn new(b: &Behavior) -> Self {
        let anim = match b.animation.as_deref().unwrap_or("") {
            "pulse" | "breathe" => Anim::Pulse,
            "blink" => Anim::Blink,
            "spin" | "rotate" => Anim::Spin,
            "marquee" | "scroll" => Anim::Marquee,
            "fade_in" | "fade" => Anim::FadeIn,
            "slide_up" | "slide" => Anim::SlideUp,
            "" | "none" => Anim::None,
            other => {
                log::warn!("animação desconhecida '{other}'");
                Anim::None
            }
        };
        Self {
            visible: b.visible_if.as_deref().filter(|s| !s.trim().is_empty()).and_then(|s| parse_expr(s, "visible_if")),
            opacity: b.layer_opacity.clamp(0.0, 1.0),
            anim,
            speed: if b.animation_speed > 0.0 { b.animation_speed } else { 1.0 },
            glow: b.glow.as_deref().map(|g| (color(g), if b.glow_radius > 0.0 { b.glow_radius } else { 6.0 })),
            rules: b
                .color_rules
                .iter()
                .filter_map(|r| Some((parse_expr(&r.when, "color_rules")?, color(&r.color))))
                .collect(),
        }
    }

    fn is_dynamic(&self) -> bool {
        self.visible.is_some() || !self.rules.is_empty()
    }
}

struct TextWidget {
    def: TextDef,
    tpl: Template,
    font: FontArc,
    color: Color,
    shadow: Option<Color>,
    outline: Option<Color>,
    gradient: Vec<Color>,
    cache: Option<(String, TextMask)>,
    last_pos: Option<[f32; 4]>,
}

struct GaugeWidget {
    def: GaugeDef,
    metric: Option<Expr>,
    stops: Vec<Color>,
    track: Color,
    value: Smoothed,
}

struct BarWidget {
    def: BarDef,
    metric: Option<Expr>,
    stops: Vec<Color>,
    track: Color,
    value: Smoothed,
}

struct SparklineWidget {
    def: SparklineDef,
    color: Color,
    pts: Vec<(f32, f32)>,
}

struct ImageWidget {
    def: ImageDef,
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

struct RectWidget {
    def: RectDef,
    color: Color,
}

struct ShapeWidget {
    def: ShapeDef,
    metric: Option<Expr>,
    stops: Vec<Color>,
    stroke: Option<Color>,
    value: Smoothed,
}

struct CoresWidget {
    def: CoresDef,
    stops: Vec<Color>,
    track: Color,
    values: Vec<Smoothed>,
}

enum Kind {
    Text(TextWidget),
    Gauge(GaugeWidget),
    Bar(BarWidget),
    Sparkline(SparklineWidget),
    Image(ImageWidget),
    Rect(RectWidget),
    Shape(ShapeWidget),
    Cores(CoresWidget),
}

/// Uma camada do tema: conteúdo + comportamentos.
pub struct Widget {
    kind: Kind,
    beh: BehaviorRt,
    z: i32,
    visible: bool,
}

impl Widget {
    pub fn z(&self) -> i32 {
        self.z
    }

    /// Constrói todas as camadas mantendo a ordem de declaração dentro de cada z.
    pub fn build_all(defs: &WidgetDefs, fonts: &mut FontStore, theme_dir: &Path, palette: &Palette, default_font: Option<&str>) -> Vec<Widget> {
        PALETTE.with(|p| *p.borrow_mut() = palette.clone());
        let mut v: Vec<Widget> = Vec::new();
        let mut push = |kind: Kind, b: &Behavior, z: i32| v.push(Widget { kind, beh: BehaviorRt::new(b), z, visible: true });
        let metric = |m: &str| parse_expr(m, "metric");

        for d in &defs.shape {
            let fill = color(&d.fill);
            push(
                Kind::Shape(ShapeWidget {
                    metric: d.metric.as_deref().filter(|m| !m.is_empty()).and_then(metric),
                    stops: colors(&d.fill_gradient, fill),
                    stroke: d.stroke.as_deref().map(color),
                    def: d.clone(),
                    value: Smoothed::default(),
                }),
                &d.behavior,
                d.z,
            );
        }
        for d in &defs.rect {
            push(Kind::Rect(RectWidget { color: color(&d.color_hex), def: d.clone() }), &d.behavior, d.z);
        }
        for d in &defs.image {
            match load_image(theme_dir, d) {
                Ok(w) => push(Kind::Image(w), &d.behavior, d.z),
                Err(e) => log::warn!("imagem '{}': {e}", d.path),
            }
        }
        for d in &defs.sparkline {
            push(Kind::Sparkline(SparklineWidget { color: color(&d.color_hex), def: d.clone(), pts: Vec::new() }), &d.behavior, d.z);
        }
        for d in &defs.cores {
            push(
                Kind::Cores(CoresWidget {
                    stops: colors(&d.color_gradient, Color::WHITE),
                    track: color(&d.track_color),
                    def: d.clone(),
                    values: Vec::new(),
                }),
                &d.behavior,
                d.z,
            );
        }
        for d in &defs.bar {
            let base = d.color_hex.as_deref().map(color).unwrap_or(Color::WHITE);
            push(
                Kind::Bar(BarWidget {
                    metric: metric(&d.metric),
                    stops: colors(&d.color_gradient, base),
                    track: color(&d.track_color),
                    def: d.clone(),
                    value: Smoothed::default(),
                }),
                &d.behavior,
                d.z,
            );
        }
        for d in &defs.radial_gauge {
            push(
                Kind::Gauge(GaugeWidget {
                    metric: metric(&d.metric),
                    stops: colors(&d.color_gradient, Color::WHITE),
                    track: color(&d.track_color),
                    def: d.clone(),
                    value: Smoothed::default(),
                }),
                &d.behavior,
                d.z,
            );
        }
        for d in &defs.text {
            let Some(font) = fonts.get(d.font.as_deref().or(default_font)) else {
                log::error!("nenhuma fonte disponível para texto '{}'", d.text);
                continue;
            };
            push(
                Kind::Text(TextWidget {
                    tpl: Template::parse(&d.text),
                    font,
                    color: color(&d.color_hex).with_alpha_mul(d.opacity),
                    shadow: d.shadow.as_deref().map(|s| color(s).with_alpha_mul(d.opacity)),
                    outline: d.outline.as_deref().map(|s| color(s).with_alpha_mul(d.opacity)),
                    gradient: d.gradient.iter().map(|g| color(g).with_alpha_mul(d.opacity)).collect(),
                    def: d.clone(),
                    cache: None,
                    last_pos: None,
                }),
                &d.behavior,
                d.z,
            );
        }
        // Ordenação estável: z crescente; empate mantém a ordem acima (fundo→frente).
        v.sort_by_key(|w| w.z);
        v
    }

    pub fn uses_seconds(&self) -> bool {
        // Condições podem depender do relógio/telemetria: reavaliar a cada segundo.
        self.beh.is_dynamic() || matches!(&self.kind, Kind::Text(t) if t.tpl.uses_seconds)
    }

    pub fn uses_time(&self) -> bool {
        self.beh.is_dynamic() || matches!(&self.kind, Kind::Text(t) if t.tpl.uses_time)
    }

    /// Desenha a camada aplicando comportamentos. `scratch` é uma camada
    /// temporária usada para glow/opacidade/deslocamentos.
    /// Retorna `true` se ainda está animando (precisa de novos quadros).
    pub fn draw(&mut self, c: &mut Canvas, scratch: &mut Canvas, ctx: &FrameCtx) -> bool {
        let env = &ctx.env;
        self.visible = self.beh.visible.as_ref().is_none_or(|e| env.eval(e).is_some_and(|v| v != 0.0));
        let continuous = self.beh.anim != Anim::None
            && !(matches!(self.beh.anim, Anim::FadeIn | Anim::SlideUp) && env.t * self.beh.speed > 1.0);
        if !self.visible {
            return continuous;
        }

        let t = env.t * self.beh.speed;
        let mut ov = Overrides {
            color: self.beh.rules.iter().find(|(e, _)| env.eval(e).is_some_and(|v| v != 0.0)).map(|(_, c)| *c),
            ..Default::default()
        };
        let mut alpha = self.beh.opacity;
        let mut dy = 0i32;
        let ease = |x: f32| 1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3);
        match self.beh.anim {
            Anim::Pulse => alpha *= 0.55 + 0.45 * (0.5 + 0.5 * (t * std::f32::consts::TAU).sin()),
            Anim::Blink => {
                if (t * 2.0).floor() as i64 % 2 == 1 {
                    alpha *= 0.12;
                }
            }
            Anim::Spin => ov.rotation = (t * 90.0) % 360.0,
            Anim::Marquee => ov.marquee = Some(t * 30.0),
            Anim::FadeIn => alpha *= ease(t / 0.8),
            Anim::SlideUp => {
                let k = ease(t / 0.8);
                alpha *= k;
                dy = ((1.0 - k) * 18.0).round() as i32;
            }
            Anim::None => {}
        }

        let needs_layer = self.beh.glow.is_some() || alpha < 0.999 || dy != 0;
        let anim_kind = if needs_layer {
            scratch.clear(0);
            let a = self.kind.draw(scratch, ctx, &ov);
            if let Some((gc, gr)) = self.beh.glow {
                c.glow_from(scratch, gc, gr, alpha, dy);
            }
            c.composite_alpha(scratch, alpha, dy);
            a
        } else {
            self.kind.draw(c, ctx, &ov)
        };
        anim_kind || continuous
    }

    /// Caixa ocupada pela camada (após o último desenho), para o editor visual.
    pub fn bounds(&self) -> [f32; 4] {
        match &self.kind {
            Kind::Rect(w) => [w.def.x, w.def.y, w.def.width, w.def.height],
            Kind::Shape(w) => [w.def.x, w.def.y, w.def.width, w.def.height],
            Kind::Bar(w) => [w.def.x, w.def.y, w.def.width, w.def.height],
            Kind::Sparkline(w) => [w.def.x, w.def.y, w.def.width, w.def.height],
            Kind::Cores(w) => [w.def.x, w.def.y, w.def.width, w.def.height],
            Kind::Image(w) => [w.def.x, w.def.y, w.w as f32, w.h as f32],
            Kind::Gauge(w) => {
                let r = w.def.radius + w.def.stroke_width / 2.0;
                [w.def.center_x - r, w.def.center_y - r, 2.0 * r, 2.0 * r]
            }
            Kind::Text(w) => match &w.last_pos {
                Some(b) => *b,
                None => [w.def.x, w.def.y, 0.0, w.def.size],
            },
        }
    }
}

impl Kind {
    fn draw(&mut self, c: &mut Canvas, ctx: &FrameCtx, ov: &Overrides) -> bool {
        let env = &ctx.env;
        match self {
            Kind::Rect(w) => {
                let d = &w.def;
                c.fill_round_rect(d.x, d.y, d.width, d.height, d.radius, ov.color.unwrap_or(w.color));
                false
            }
            Kind::Image(w) => {
                c.blit_rgba(&w.px, w.w, w.h, w.def.x.round() as i32, w.def.y.round() as i32, w.def.opacity);
                false
            }
            Kind::Text(w) => {
                w.draw(c, ctx, ov);
                false
            }
            Kind::Gauge(w) => {
                let d = &w.def;
                let target = norm(w.metric.as_ref().and_then(|e| env.eval(e)), d.min, d.max);
                let (t, anim) = w.value.step(target, ctx.dt, d.smooth);
                c.arc(d.center_x, d.center_y, d.radius, d.stroke_width, d.start_angle, d.end_angle, d.rounded_caps, |_| w.track);
                if t > 0.001 {
                    let end = d.start_angle + (d.end_angle - d.start_angle) * t;
                    let single = ov.color.map(|c| vec![c]);
                    let stops: &[Color] = single.as_deref().unwrap_or(&w.stops);
                    let by_value = d.color_mode.eq_ignore_ascii_case("value");
                    let vcol = Color::gradient(stops, t);
                    c.arc(d.center_x, d.center_y, d.radius, d.stroke_width, d.start_angle, end, d.rounded_caps, |p| {
                        if by_value { vcol } else { Color::gradient(stops, p * t) }
                    });
                }
                anim
            }
            Kind::Bar(w) => {
                let d = &w.def;
                let target = norm(w.metric.as_ref().and_then(|e| env.eval(e)), d.min, d.max);
                let (t, anim) = w.value.step(target, ctx.dt, d.smooth);
                c.fill_round_rect(d.x, d.y, d.width, d.height, d.radius, w.track);
                let col = ov.color.unwrap_or_else(|| Color::gradient(&w.stops, t));
                if d.orientation.eq_ignore_ascii_case("vertical") {
                    let h = d.height * t;
                    c.fill_round_rect(d.x, d.y + d.height - h, d.width, h, d.radius, col);
                } else {
                    c.fill_round_rect(d.x, d.y, d.width * t, d.height, d.radius, col);
                }
                anim
            }
            Kind::Sparkline(w) => {
                w.draw(c, ctx, ov);
                false
            }
            Kind::Shape(w) => w.draw(c, ctx, ov),
            Kind::Cores(w) => w.draw(c, ctx, ov),
        }
    }
}

impl TextWidget {
    fn draw(&mut self, c: &mut Canvas, ctx: &FrameCtx, ov: &Overrides) {
        let d = &self.def;
        let mut s = self.tpl.render(&ctx.env);
        if d.uppercase {
            s = s.to_uppercase();
        }
        let marquee = ov.marquee.is_some();
        let stale = self.cache.as_ref().is_none_or(|(old, _)| *old != s);
        if stale {
            let key = s.clone();
            if let (Some(maxw), false) = (d.max_width, marquee) {
                // Trunca com reticências até caber.
                let fits = |t: &str| measure(t, &self.font, ctx.fallbacks, d.size, d.letter_spacing) <= maxw;
                if !fits(&s) {
                    let mut chars: Vec<char> = s.chars().collect();
                    while !chars.is_empty() {
                        chars.pop();
                        let cand: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
                        if fits(&cand) {
                            s = cand;
                            break;
                        }
                    }
                }
            }
            let mask = render_text(&s, &self.font, ctx.fallbacks, d.size, d.letter_spacing);
            self.cache = Some((key, mask));
        }
        let (_, m) = self.cache.as_ref().unwrap();

        // Letreiro: rola o texto dentro de `max_width` quando ele não cabe.
        let clip_w = d.max_width.unwrap_or(m.w as f32);
        let scrolling = marquee && (m.w as f32) > clip_w;
        let box_w = if scrolling { clip_w } else { m.w as f32 };
        let x = match d.align.as_str() {
            "center" | "centre" => d.x - box_w / 2.0,
            "right" => d.x - box_w,
            _ => d.x,
        }
        .round() as i32;
        let y = match d.valign.as_str() {
            "middle" | "center" => d.y - m.h as f32 / 2.0,
            "bottom" => d.y - m.h as f32,
            _ => d.y,
        }
        .round() as i32;
        self.last_pos = Some([x as f32, y as f32, box_w, m.h as f32]);

        let clip = (x, x + box_w.ceil() as i32);
        let offsets: Vec<i32> = if scrolling {
            let period = m.w as f32 + 40.0;
            let off = ov.marquee.unwrap_or(0.0) % period;
            vec![x - off as i32, x - off as i32 + period as i32]
        } else {
            vec![x]
        };
        let fill = ov.color.unwrap_or(self.color);
        let grad: Option<&[Color]> = if ov.color.is_none() && self.gradient.len() > 1 { Some(&self.gradient) } else { None };
        for ox in offsets {
            if let Some(sh) = self.shadow {
                c.draw_mask_ex(&m.data, m.w, m.h, ox + 1, y + 1, clip, |_| sh);
            }
            if let Some(oc) = self.outline {
                let r = if d.outline_width > 0.0 { d.outline_width } else { 1.0 };
                let steps = ((r * 6.0).ceil() as i32).clamp(8, 24);
                for k in 0..steps {
                    let a = k as f32 / steps as f32 * std::f32::consts::TAU;
                    let (dx, dy) = ((a.cos() * r).round() as i32, (a.sin() * r).round() as i32);
                    c.draw_mask_ex(&m.data, m.w, m.h, ox + dx, y + dy, clip, |_| oc);
                }
            }
            match grad {
                Some(g) => c.draw_mask_ex(&m.data, m.w, m.h, ox, y, clip, |row| Color::gradient(g, row as f32 / m.h.max(1) as f32)),
                None => c.draw_mask_ex(&m.data, m.w, m.h, ox, y, clip, |_| fill),
            }
        }
    }
}

impl SparklineWidget {
    fn draw(&mut self, c: &mut Canvas, ctx: &FrameCtx, ov: &Overrides) {
        let d = &self.def;
        let Some(hist) = ctx.env.snap.history(&d.metric) else { return };
        let n = d.seconds.max(2).min(hist.len());
        if n < 2 {
            return;
        }
        let data: Vec<f32> = hist.iter().skip(hist.len() - n).copied().collect();
        let lo = d.min.map(|v| v as f32).unwrap_or(0.0);
        let hi = d
            .max
            .map(|v| v as f32)
            .unwrap_or_else(|| data.iter().copied().fold(1.0f32, f32::max) * 1.15);
        let span = (hi - lo).max(1e-6);
        let step = d.width / (d.seconds.max(2) - 1) as f32;
        // Alinha à direita: a amostra mais recente fica na borda direita.
        let x_end = d.x + d.width;
        self.pts.clear();
        for (i, v) in data.iter().enumerate() {
            let x = x_end - (n - 1 - i) as f32 * step;
            let t = ((v - lo) / span).clamp(0.0, 1.0);
            let y = d.y + d.height - t * (d.height - d.stroke_width) - d.stroke_width / 2.0;
            self.pts.push((x, y));
        }
        let col = ov.color.unwrap_or(self.color);
        if d.fill_opacity > 0.0 {
            c.fill_under(&self.pts, d.y + d.height, col.with_alpha_mul(d.fill_opacity));
        }
        c.polyline(&self.pts, d.stroke_width, col);
    }
}

impl CoresWidget {
    fn draw(&mut self, c: &mut Canvas, ctx: &FrameCtx, ov: &Overrides) -> bool {
        let d = &self.def;
        let cores = &ctx.env.snap.cpu_cores;
        let n = cores.len();
        if n == 0 {
            return false;
        }
        self.values.resize_with(n, Smoothed::default);
        let cols = if d.columns == 0 { n } else { (d.columns as usize).min(n) };
        let rows = n.div_ceil(cols);
        let cw = (d.width - d.gap * (cols as f32 - 1.0)) / cols as f32;
        let ch = (d.height - d.gap * (rows as f32 - 1.0)) / rows as f32;
        let vertical = !d.orientation.eq_ignore_ascii_case("horizontal");
        let mut anim = false;
        for (i, &u) in cores.iter().enumerate() {
            let (t, a) = self.values[i].step((u / 100.0).clamp(0.0, 1.0), ctx.dt, true);
            anim |= a;
            let (r, k) = (i / cols, i % cols);
            let x = d.x + k as f32 * (cw + d.gap);
            let y = d.y + r as f32 * (ch + d.gap);
            c.fill_round_rect(x, y, cw, ch, d.radius, self.track);
            let col = ov.color.unwrap_or_else(|| Color::gradient(&self.stops, t));
            if vertical {
                c.fill_round_rect(x, y + ch * (1.0 - t), cw, ch * t, d.radius, col);
            } else {
                c.fill_round_rect(x, y, cw * t, ch, d.radius, col);
            }
        }
        anim
    }
}

impl ShapeWidget {
    /// Gira pontos em torno de (cx, cy).
    fn rotate(pts: &mut [(f32, f32)], cx: f32, cy: f32, deg: f32) {
        if deg == 0.0 {
            return;
        }
        let (s, c) = deg.to_radians().sin_cos();
        for p in pts {
            let (dx, dy) = (p.0 - cx, p.1 - cy);
            *p = (cx + dx * c - dy * s, cy + dx * s + dy * c);
        }
    }

    /// Contorno principal da forma em coordenadas de tela (sem rotação).
    fn outline(&self) -> Vec<(f32, f32)> {
        let d = &self.def;
        let (w, h) = (d.width.max(0.0), d.height.max(0.0));
        let (cx, cy) = (d.x + w / 2.0, d.y + h / 2.0);
        let ring = |n: usize, rad: &dyn Fn(usize) -> f32, start: f32| -> Vec<(f32, f32)> {
            (0..n)
                .map(|i| {
                    let a = start + i as f32 * std::f32::consts::TAU / n as f32;
                    let r = rad(i);
                    (cx + a.cos() * w / 2.0 * r, cy + a.sin() * h / 2.0 * r)
                })
                .collect()
        };
        let top = -std::f32::consts::FRAC_PI_2;
        match d.kind.as_str() {
            "ellipse" | "circle" => ring(96, &|_| 1.0, 0.0),
            "regular" | "hexagon" | "triangle" => ring(d.sides.clamp(3, 64) as usize, &|_| 1.0, top),
            "star" => {
                let n = d.sides.clamp(2, 64) as usize * 2;
                let inner = d.inner_ratio.clamp(0.05, 1.0);
                ring(n, &|i| if i % 2 == 0 { 1.0 } else { inner }, top)
            }
            "polygon" | "line" | "path" => d.points.iter().map(|p| (d.x + p[0] * w, d.y + p[1] * h)).collect(),
            _ => {
                let r = d.corner_radius.min(w / 2.0).min(h / 2.0).max(0.0);
                if r < 0.5 {
                    vec![(d.x, d.y), (d.x + w, d.y), (d.x + w, d.y + h), (d.x, d.y + h)]
                } else {
                    let mut v = Vec::new();
                    let corners = [
                        (d.x + w - r, d.y + r, -90.0f32),
                        (d.x + w - r, d.y + h - r, 0.0),
                        (d.x + r, d.y + h - r, 90.0),
                        (d.x + r, d.y + r, 180.0),
                    ];
                    for (ccx, ccy, a0) in corners {
                        for k in 0..=8 {
                            let a = (a0 + k as f32 * 90.0 / 8.0).to_radians();
                            v.push((ccx + r * a.cos(), ccy + r * a.sin()));
                        }
                    }
                    v
                }
            }
        }
    }

    fn draw(&mut self, c: &mut Canvas, ctx: &FrameCtx, ov: &Overrides) -> bool {
        let base = self.outline();
        let d = &self.def;
        let (w, h) = (d.width.max(0.0), d.height.max(0.0));
        let (cx, cy) = (d.x + w / 2.0, d.y + h / 2.0);
        let extra: Vec<Vec<(f32, f32)>> = d
            .paths
            .iter()
            .map(|p| p.iter().map(|q| (d.x + q[0] * w, d.y + q[1] * h)).collect())
            .collect();

        let (mut anim, mut level) = (false, None);
        if let Some(m) = &self.metric {
            let target = norm(ctx.env.eval(m), d.min, d.max);
            let (t, a) = self.value.step(target, ctx.dt, true);
            anim = a;
            level = Some(t);
        }
        let opacity = d.opacity.clamp(0.0, 1.0);
        let is_line = d.kind == "line" || d.kind == "path";
        let single = ov.color.map(|c| vec![c]);
        let stops: &[Color] = single.as_deref().unwrap_or(&self.stops);
        let stroke = if is_line { ov.color.or(self.stroke).or(stops.first().copied()) } else { self.stroke };
        let sw = if d.stroke_width > 0.0 { d.stroke_width } else if is_line { 2.0 } else { 0.0 };

        let copies = d.repeat.clamp(1, 64);
        for k in 0..copies {
            let rot = d.rotation + ov.rotation + k as f32 * 360.0 / copies as f32;
            let mut pts = base.clone();
            Self::rotate(&mut pts, cx, cy, rot);

            if !is_line && pts.len() >= 3 {
                let (minx, maxx) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
                let (miny, maxy) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
                let (bx, by) = ((minx + maxx) / 2.0, (miny + maxy) / 2.0);
                let span = (maxx - minx).max(maxy - miny).max(1.0);
                // Projeção normalizada 0..1 ao longo de um ângulo (degradê linear / nível).
                let proj = |ang: f32| {
                    let (s, co) = ang.to_radians().sin_cos();
                    let half = ((maxx - minx) * co.abs() + (maxy - miny) * s.abs()) / 2.0;
                    move |x: f32, y: f32| (((x - bx) * co + (y - by) * s) / half.max(0.5) + 1.0) / 2.0
                };
                let g = proj(d.gradient_angle);
                let lv = proj(d.fill_direction);
                let radial = d.gradient_type.eq_ignore_ascii_case("radial");
                let rr = span / 2.0;
                let color_at = |x: f32, y: f32| {
                    if stops.len() == 1 {
                        stops[0]
                    } else if radial {
                        Color::gradient(stops, ((x - bx).hypot(y - by) / rr).min(1.0))
                    } else {
                        Color::gradient(stops, g(x, y))
                    }
                };
                c.fill_path(&pts, |x, y| {
                    let k = match level {
                        Some(t) => ((t - lv(x, y)) * span + 0.5).clamp(0.0, 1.0),
                        None => 1.0,
                    };
                    (k > 0.0).then(|| (color_at(x, y), k * opacity))
                });
            }
            if let (Some(sc), true) = (stroke, sw > 0.0) {
                let sc = sc.with_alpha_mul(opacity);
                if pts.len() >= 2 {
                    let mut path = pts.clone();
                    if !is_line {
                        path.push(pts[0]);
                    }
                    c.polyline_free(&path, sw, sc);
                }
                for e in &extra {
                    let mut p = e.clone();
                    Self::rotate(&mut p, cx, cy, rot);
                    c.polyline_free(&p, sw, sc);
                }
            }
        }
        anim
    }
}

fn load_image(theme_dir: &Path, d: &ImageDef) -> anyhow::Result<ImageWidget> {
    let mut img = image::open(theme_dir.join(&d.path))?.to_rgba8();
    if d.width.is_some() || d.height.is_some() {
        let (w0, h0) = img.dimensions();
        let w = d.width.unwrap_or_else(|| (w0 as f32 * d.height.unwrap() as f32 / h0 as f32).round() as u32);
        let h = d.height.unwrap_or_else(|| (h0 as f32 * w as f32 / w0 as f32).round() as u32);
        img = image::imageops::resize(&img, w.max(1), h.max(1), image::imageops::FilterType::Lanczos3);
    }
    let (w, h) = img.dimensions();
    Ok(ImageWidget { def: d.clone(), w: w as usize, h: h as usize, px: img.pixels().map(|p| p.0).collect() })
}
