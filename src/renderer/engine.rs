//! Compositor de camadas: fundo (sólido/imagem/GIF/vídeo) + widgets ordenados
//! por `z`. Roda numa thread própria, renderiza somente quando algo muda e
//! recarrega o tema automaticamente quando arquivos da pasta são alterados.

use super::canvas::{Canvas, Color, Orientation};
use super::widgets::{Behavior, Env, FontStore, FrameCtx, Palette, TextDef, Widget, WidgetDefs};
use std::collections::BTreeMap;
use crate::driver::{blank_frame, Frame565, TripleBuffer};
use crate::media::{Background, Fit};
use crate::telemetry::{Snapshot, TelemetryHub};
use anyhow::{Context, Result};
use chrono::{Local, Timelike};
use crossbeam_channel::{select, unbounded, Receiver};
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Meta {
    pub name: String,
    pub author: String,
    pub description: String,
    /// FPS máximo do tema (limitado pelo `max_fps` global).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    /// Fonte padrão dos textos que não definem `font`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
}

fn d_black() -> String { "#000000".into() }

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct BackgroundDef {
    /// `video`, `gif`, `static`, `solid` ou `auto` (pela extensão).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default = "d_black")]
    pub color: String,
    pub fit: Fit,
    /// Escurece o fundo (0..1) para aumentar o contraste dos widgets.
    pub dim: f32,
}

impl Default for BackgroundDef {
    fn default() -> Self {
        Self { kind: "auto".into(), path: None, color: d_black(), fit: Fit::Cover, dim: 0.0 }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ThemeFile {
    pub meta: Meta,
    pub background: BackgroundDef,
    /// Cores nomeadas, usadas como `$nome` em qualquer campo de cor.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub palette: BTreeMap<String, String>,
    pub widgets: WidgetDefs,
}

fn build_palette(p: &BTreeMap<String, String>) -> Palette {
    // Resolve em duas passadas para permitir `$a = "$b"`.
    let mut out = Palette::new();
    for _ in 0..2 {
        for (k, v) in p {
            if v.trim().starts_with('$') {
                if let Some(c) = out.get(v.trim().trim_start_matches('$')).copied() {
                    out.insert(k.clone(), c);
                }
            } else if let Some(c) = Color::parse(v) {
                out.insert(k.clone(), c);
            }
        }
    }
    out
}

pub struct Theme {
    palette: Palette,
    default_font: Option<String>,
    start: Instant,
    scratch: Canvas,
    dir: PathBuf,
    fonts_dir: PathBuf,
    cache_bytes: usize,
    bg_def: BackgroundDef,
    /// Camada de widgets em cache (ARGB pré-multiplicado).
    overlay: Canvas,
    pub bg: Background,
    pub dim: f32,
    pub widgets: Vec<Widget>,
    pub fps: Option<u32>,
    fallbacks: Vec<ab_glyph::FontArc>,
    uses_seconds: bool,
    uses_time: bool,
}

impl Theme {
    pub fn read_file(dir: &Path) -> Result<ThemeFile> {
        let path = dir.join("theme.toml");
        let src = std::fs::read_to_string(&path).with_context(|| format!("lendo {}", path.display()))?;
        toml::from_str(&src).with_context(|| format!("sintaxe de {}", path.display()))
    }

    pub fn load(dir: &Path, fonts_dir: &Path, cache_bytes: usize) -> Result<Self> {
        Self::from_file(&Self::read_file(dir)?, dir, fonts_dir, cache_bytes)
    }

    fn load_background(def: &BackgroundDef, dir: &Path, cache_bytes: usize) -> Result<Background> {
        let bg_color = Color::parse(&def.color).unwrap_or(Color::BLACK).rgb();
        let bg_path = def.path.as_ref().filter(|p| !p.is_empty()).map(|p| dir.join(p));
        Background::load(&def.kind, bg_path.as_deref(), bg_color, def.fit, cache_bytes).with_context(|| "carregando background")
    }

    /// Constrói o tema a partir de uma definição em memória (usado pelo editor).
    pub fn from_file(file: &ThemeFile, dir: &Path, fonts_dir: &Path, cache_bytes: usize) -> Result<Self> {
        let bg = Self::load_background(&file.background, dir, cache_bytes)?;

        let mut fonts = FontStore::new(dir, fonts_dir);
        let palette = build_palette(&file.palette);
        let widgets = Widget::build_all(&file.widgets, &mut fonts, dir, &palette, file.meta.font.as_deref());
        let name = if file.meta.name.is_empty() {
            dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            file.meta.name.clone()
        };
        log::info!("tema '{name}' carregado: {} widgets", widgets.len());
        Ok(Self {
            palette,
            default_font: file.meta.font.clone(),
            start: Instant::now(),
            scratch: Canvas::new(),
            dir: dir.to_path_buf(),
            fonts_dir: fonts_dir.to_path_buf(),
            cache_bytes,
            bg_def: file.background.clone(),
            overlay: Canvas::new(),
            uses_seconds: widgets.iter().any(Widget::uses_seconds),
            uses_time: widgets.iter().any(Widget::uses_time),
            bg,
            dim: file.background.dim,
            widgets,
            fps: file.meta.fps,
            fallbacks: fonts.fallbacks().to_vec(),
        })
    }

    /// Tema de emergência exibindo a mensagem de erro no próprio display.
    pub fn error(dir: &Path, fonts_dir: &Path, err: &anyhow::Error) -> Self {
        let mut fonts = FontStore::new(dir, fonts_dir);
        let msg = format!("{err:#}");
        let mut defs = WidgetDefs::default();
        let mk = |text: String, y: f32, size: f32, color: &str| TextDef {
            text,
            font: None,
            size,
            x: 160.0,
            y,
            color_hex: color.into(),
            align: "center".into(),
            valign: String::new(),
            max_width: Some(300.0),
            shadow: None,
            letter_spacing: 0.0,
            opacity: 1.0,
            outline: None,
            outline_width: 0.0,
            gradient: Vec::new(),
            uppercase: false,
            z: 0,
            behavior: Behavior::default(),
        };
        defs.text.push(mk("ERRO NO TEMA".into(), 70.0, 22.0, "#FF5555"));
        for (i, chunk) in wrap(&msg, 42).into_iter().take(5).enumerate() {
            // Escapa chaves para a mensagem não ser interpretada como placeholder.
            defs.text.push(mk(chunk.replace('{', "{{").replace('}', "}}"), 110.0 + i as f32 * 18.0, 13.0, "#FFFFFF"));
        }
        defs.text.push(mk("{time:%H:%M:%S}".into(), 210.0, 14.0, "#888888"));
        let widgets = Widget::build_all(&defs, &mut fonts, dir, &Palette::new(), None);
        Self {
            palette: Palette::new(),
            default_font: None,
            start: Instant::now(),
            scratch: Canvas::new(),
            dir: dir.to_path_buf(),
            fonts_dir: fonts_dir.to_path_buf(),
            cache_bytes: 0,
            bg_def: BackgroundDef::default(),
            overlay: Canvas::new(),
            bg: Background::Solid(0x1A0000),
            dim: 0.0,
            widgets,
            fps: None,
            fallbacks: fonts.fallbacks().to_vec(),
            uses_seconds: true,
            uses_time: true,
        }
    }

    /// Substitui o fundo, recarregando a mídia apenas se a definição mudou.
    pub fn set_background(&mut self, def: &BackgroundDef) -> Result<()> {
        let media_changed = def.kind != self.bg_def.kind || def.path != self.bg_def.path || def.fit != self.bg_def.fit || def.color != self.bg_def.color;
        if media_changed {
            self.bg = Self::load_background(def, &self.dir, self.cache_bytes)?;
        }
        self.dim = def.dim;
        self.bg_def = def.clone();
        Ok(())
    }

    /// Aplica uma definição completa (editor): fundo, paleta, fonte padrão e camadas.
    pub fn apply(&mut self, file: &ThemeFile) -> Result<()> {
        self.set_background(&file.background)?;
        self.palette = build_palette(&file.palette);
        self.default_font = file.meta.font.clone();
        self.fps = file.meta.fps;
        self.set_widgets(&file.widgets);
        Ok(())
    }

    /// Reinicia o relógio das animações (`t = 0`).
    pub fn restart_animations(&mut self) {
        self.start = Instant::now();
    }

    /// Reconstrói todos os widgets (o fundo é preservado).
    pub fn set_widgets(&mut self, defs: &WidgetDefs) {
        let mut fonts = FontStore::new(&self.dir, &self.fonts_dir);
        self.widgets = Widget::build_all(defs, &mut fonts, &self.dir, &self.palette, self.default_font.as_deref());
        self.fallbacks = fonts.fallbacks().to_vec();
        self.uses_seconds = self.widgets.iter().any(Widget::uses_seconds);
        self.uses_time = self.widgets.iter().any(Widget::uses_time);
    }

    /// Caixas dos widgets na ordem de desenho (z crescente).
    pub fn widget_bounds(&self) -> Vec<[f32; 4]> {
        self.widgets.iter().map(Widget::bounds).collect()
    }

    /// Redesenha a camada de widgets. Retorna `true` se algum widget ainda anima.
    pub fn update_overlay(&mut self, snap: &Snapshot, dt: f32) -> bool {
        let env = Env { snap, now: Local::now(), t: self.start.elapsed().as_secs_f32() };
        let ctx = FrameCtx { env, dt, fallbacks: &self.fallbacks };
        self.overlay.clear(0);
        let mut animating = false;
        for w in &mut self.widgets {
            animating |= w.draw(&mut self.overlay, &mut self.scratch, &ctx);
        }
        animating
    }

    /// Fundo (+ escurecimento) + camada de widgets em cache.
    pub fn compose(&mut self, canvas: &mut Canvas) {
        self.bg.draw(canvas);
        if self.dim > 0.0 {
            canvas.dim(self.dim);
        }
        canvas.composite(&self.overlay);
    }
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[derive(Debug, Clone)]
pub enum RenderCmd {
    SwitchTheme(PathBuf),
    Reload,
    Silent(bool),
    Orientation(Orientation),
    MaxFps(u32),
    Quit,
}

pub struct RenderSettings {
    pub theme_dir: PathBuf,
    pub fonts_dir: PathBuf,
    pub max_fps: u32,
    pub cache_bytes: usize,
    pub silent: bool,
    pub orientation: Orientation,
    /// Tela de inicialização: (pasta do tema, duração).
    pub splash: Option<(PathBuf, Duration)>,
}

fn load_or_error(dir: &Path, s: &RenderSettings) -> Theme {
    Theme::load(dir, &s.fonts_dir, s.cache_bytes).unwrap_or_else(|e| {
        log::error!("tema {}: {e:#}", dir.display());
        Theme::error(dir, &s.fonts_dir, &e)
    })
}

/// Loop principal de renderização.
pub fn run_render_thread(
    mut settings: RenderSettings,
    hub: Arc<TelemetryHub>,
    frames: Arc<TripleBuffer>,
    cmds: Receiver<RenderCmd>,
) {
    let (fs_tx, fs_rx) = unbounded::<()>();
    let make_watcher = |dir: &Path, fonts: &Path| {
        let tx = fs_tx.clone();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if !matches!(ev.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(());
                }
            }
        })
        .ok()?;
        w.watch(dir, RecursiveMode::Recursive).ok()?;
        let _ = w.watch(fonts, RecursiveMode::Recursive);
        Some(w)
    };

    // Tela de inicialização (Dark Hub por padrão): um tema comum exibido por
    // alguns segundos antes do tema ativo.
    let mut splash_until = None;
    let mut theme = match settings.splash.clone().filter(|(d, _)| d.join("theme.toml").is_file()) {
        Some((dir, dur)) if !settings.silent => match Theme::load(&dir, &settings.fonts_dir, settings.cache_bytes) {
            Ok(t) => {
                splash_until = Some(Instant::now() + dur);
                t
            }
            Err(e) => {
                log::warn!("tela de inicialização: {e:#}");
                load_or_error(&settings.theme_dir, &settings)
            }
        },
        _ => load_or_error(&settings.theme_dir, &settings),
    };
    let mut _watcher = make_watcher(&settings.theme_dir, &settings.fonts_dir);

    let mut canvas = Canvas::new();
    let mut back: Frame565 = blank_frame();
    let mut last_ver = u64::MAX;
    let mut last_clock = u32::MAX;
    let mut last_frame = Instant::now() - Duration::from_secs(1);
    let mut animating = false;
    let mut reload_at: Option<Instant> = None;
    let mut dirty = true;
    let mut silent_sent = false;
    let mut bg_dirty = true;
    let mut last_overlay = Instant::now();
    let (mut stat_since, mut stat_cpu, mut stat_frames) = (Instant::now(), crate::driver::thread_cpu_time(), 0u32);

    loop {
        let interval = Duration::from_secs_f64(
            1.0 / theme.fps.unwrap_or(settings.max_fps).min(settings.max_fps).max(1) as f64,
        );
        let now = Instant::now();

        // Próximo instante em que precisamos acordar.
        let mut wake = now + Duration::from_millis(100); // polling barato da telemetria
        if let Some(t) = theme.bg.next_deadline() {
            wake = wake.min(t);
        }
        if animating {
            wake = wake.min(last_frame + interval);
        }
        if let Some(t) = reload_at {
            wake = wake.min(t);
        }
        if let Some(t) = splash_until {
            wake = wake.min(t);
        }
        if settings.silent {
            wake = now + Duration::from_secs(1);
        }
        let timeout = wake.saturating_duration_since(now);

        select! {
            recv(cmds) -> msg => match msg {
                Ok(RenderCmd::Quit) | Err(_) => return,
                Ok(RenderCmd::SwitchTheme(dir)) => {
                    splash_until = None;
                    settings.theme_dir = dir;
                    theme = load_or_error(&settings.theme_dir, &settings);
                    _watcher = make_watcher(&settings.theme_dir, &settings.fonts_dir);
                    dirty = true;
                }
                Ok(RenderCmd::Reload) => reload_at = Some(Instant::now()),
                Ok(RenderCmd::MaxFps(f)) => {
                    settings.max_fps = f.clamp(1, crate::config::MAX_FPS_LIMIT);
                    log::info!("FPS máximo: {}", settings.max_fps);
                }
                Ok(RenderCmd::Orientation(o)) => {
                    settings.orientation = o;
                    dirty = true;
                }
                Ok(RenderCmd::Silent(on)) => {
                    settings.silent = on;
                    silent_sent = false;
                    dirty = true;
                }
            },
            recv(fs_rx) -> _ => {
                // Debounce: editores salvam em várias etapas.
                while fs_rx.try_recv().is_ok() {}
                reload_at = Some(Instant::now() + Duration::from_millis(300));
            },
            default(timeout) => {}
        }

        let now = Instant::now();
        if splash_until.is_some_and(|t| now >= t) {
            splash_until = None;
            theme = load_or_error(&settings.theme_dir, &settings);
            dirty = true;
        }
        if reload_at.is_some_and(|t| now >= t) && splash_until.is_none() {
            reload_at = None;
            log::info!("hot-reload: {}", settings.theme_dir.display());
            theme = load_or_error(&settings.theme_dir, &settings);
            dirty = true;
        }

        if settings.silent {
            if !silent_sent {
                canvas.clear(0);
                canvas.to_rgb565_be(&mut back);
                frames.publish(&mut back);
                silent_sent = true;
            }
            continue;
        }

        bg_dirty |= theme.bg.advance(now);
        let ver = hub.version();
        if ver != last_ver {
            last_ver = ver;
            dirty = true;
        }
        if theme.uses_time {
            let t = Local::now();
            let key = if theme.uses_seconds { t.num_seconds_from_midnight() } else { t.num_seconds_from_midnight() / 60 };
            if key != last_clock {
                last_clock = key;
                dirty = true;
            }
        }
        if !(dirty || bg_dirty || animating) || now.duration_since(last_frame) < interval.mul_f32(0.9) {
            continue;
        }

        // Widgets só são redesenhados quando dados/relógio mudam ou animam;
        // frames de vídeo/GIF apenas recompõem a camada em cache.
        if dirty || animating {
            let dt = now.duration_since(last_overlay).as_secs_f32().min(0.5);
            last_overlay = now;
            let snap = hub.get();
            animating = theme.update_overlay(&snap, dt);
        }
        theme.compose(&mut canvas);
        bg_dirty = false;
        canvas.to_rgb565_be_oriented(&mut back, settings.orientation);
        frames.publish(&mut back);
        last_frame = now;
        dirty = false;
        stat_frames += 1;
        if stat_since.elapsed() >= Duration::from_secs(10) {
            let cpu = crate::driver::thread_cpu_time();
            log::debug!(
                "render: {:.1} fps, CPU da thread {:.2}% de um núcleo",
                stat_frames as f64 / stat_since.elapsed().as_secs_f64(),
                (cpu - stat_cpu).as_secs_f64() / stat_since.elapsed().as_secs_f64() * 100.0
            );
            (stat_since, stat_cpu, stat_frames) = (Instant::now(), cpu, 0);
        }
    }
}

/// Grava `seconds` de animação do tema em GIF (20 fps), como o display mostraria.
pub fn record_gif(theme_dir: &Path, fonts_dir: &Path, out: &Path, hub: Arc<TelemetryHub>, seconds: f32, scale: u32) -> Result<()> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};
    // Espera a telemetria encher antes de começar a gravar.
    std::thread::sleep(Duration::from_millis(1500));
    let mut theme = Theme::load(theme_dir, fonts_dir, 64 << 20)?;
    theme.restart_animations();
    let mut canvas = Canvas::new();
    let mut enc = GifEncoder::new_with_speed(std::fs::File::create(out)?, 10);
    enc.set_repeat(Repeat::Infinite)?;
    let step = Duration::from_millis(50);
    let n = (seconds * 20.0).ceil() as u32;
    let s = scale.clamp(1, 4);
    let mut next = Instant::now();
    for _ in 0..n {
        theme.bg.advance(Instant::now());
        theme.update_overlay(&hub.get(), 0.05);
        theme.compose(&mut canvas);
        let mut img = RgbaImage::new(320 * s, 240 * s);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let p = super::canvas::rgb565_to_u32(super::canvas::u32_to_rgb565(canvas.pixels[(y / s * 320 + x / s) as usize]));
            *px = image::Rgba([(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
        }
        enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_saturating_duration(step)))?;
        next += step;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    println!("GIF salvo em {} ({n} quadros)", out.display());
    Ok(())
}

/// Renderiza o tema em PNG sem hardware (útil para criar/depurar temas).
pub fn preview(theme_dir: &Path, fonts_dir: &Path, out: &Path, hub: Arc<TelemetryHub>, warmup: Duration) -> Result<()> {
    let mut theme = Theme::load(theme_dir, fonts_dir, 64 << 20)?;
    let mut canvas = Canvas::new();
    let start = Instant::now();
    while start.elapsed() < warmup {
        theme.bg.advance(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    theme.bg.advance(Instant::now());
    let snap = hub.get();

    // Mede o custo de composição + conversão.
    let n = 60;
    let mut frame = blank_frame();
    let t = Instant::now();
    for _ in 0..n {
        theme.update_overlay(&snap, 10.0);
    }
    let per_overlay = t.elapsed() / n;
    let t = Instant::now();
    for _ in 0..n {
        theme.compose(&mut canvas);
        canvas.to_rgb565_be(&mut frame);
    }
    let per = t.elapsed() / n;
    println!(
        "widgets: {per_overlay:?} (só quando dados mudam, {} widgets) | fundo+composição+RGB565: {per:?} por quadro",
        theme.widgets.len()
    );

    // Salva o resultado como o display o verá (passando por RGB565).
    let mut img = image::RgbImage::new(320, 240);
    for (i, px) in img.pixels_mut().enumerate() {
        let v = u16::from_be_bytes([frame[2 * i], frame[2 * i + 1]]);
        let c = super::canvas::rgb565_to_u32(v);
        *px = image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8]);
    }
    img.save(out)?;
    println!("preview salvo em {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip_with_behaviors() {
        let src = r##"
[meta]
name = "t"
font = "segoeui.ttf"

[palette]
accent = "#00FFAA"
alias = "$accent"

[[widgets.text]]
text = "{cpu_temp*1.8+32:.0}°F"
x = 10
y = 10
visible_if = "hour >= 0"
animation = "marquee"
glow = "$accent"
color_rules = [{ when = "cpu_temp > 80", color = "#FF0000" }]

[[widgets.shape]]
kind = "line"
x = 0
y = 0
width = 100
height = 100
points = [[0.5, 0.5], [0.5, 0.0]]
paths = [[[0.4, 0.2], [0.5, 0.3], [0.6, 0.2]]]
repeat = 6
"##;
        let f: ThemeFile = toml::from_str(src).unwrap();
        assert_eq!(f.widgets.text[0].behavior.animation.as_deref(), Some("marquee"));
        assert_eq!(f.widgets.text[0].behavior.color_rules.len(), 1);
        assert_eq!(f.widgets.shape[0].repeat, 6);
        assert_eq!(build_palette(&f.palette).get("alias"), Some(&Color::parse("#00FFAA").unwrap()));
        let back = toml::to_string_pretty(&f).unwrap();
        let again: ThemeFile = toml::from_str(&back).unwrap();
        assert_eq!(again.widgets.text[0].behavior.glow.as_deref(), Some("$accent"));
        assert_eq!(again.widgets.shape[0].paths.len(), 1);
        assert!(!back.contains("layer_opacity"), "valores padrão não devem poluir o TOML");
    }
}
