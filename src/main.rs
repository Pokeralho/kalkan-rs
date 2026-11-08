//! Kalkan-RS — painel e driver de alta performance para o display AIO
//! KALKAN / GAMDIAS ZEUS CAST (320x240 RGB565 via LcdComm.dll).
//!
//! Threads:
//! * principal  – System Tray + loop de mensagens Win32
//! * telemetry  – coleta a 1 Hz (sysinfo, NVML, PDH, WMI, WinRT)
//! * render     – compositor de camadas, hot-reload, publica no triple buffer
//! * driver     – envia frames ao USB, keep-alive e auto-reconexão
//! * decoders   – vídeo (Media Foundation) / animações (GIF/WebP/APNG)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kalkan_rs::config::Config;
use kalkan_rs::{driver, renderer, telemetry, ui};
use crossbeam_channel::unbounded;
use driver::lcd_comm::{self, DriverStatus};
use driver::{DriverCmd, TripleBuffer};
use renderer::engine::{self, RenderCmd, RenderSettings};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telemetry::TelemetryHub;

// ───────────────────────────── Logger ─────────────────────────────

struct Logger {
    level: log::LevelFilter,
    file: Option<Mutex<std::fs::File>>,
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        // Logs de dependências (wmi, notify…) só a partir de WARN.
        m.level() <= self.level && (m.target().starts_with("kalkan_rs") || m.level() <= log::Level::Warn)
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("{} {:<5} {}\n", chrono::Local::now().format("%H:%M:%S%.3f"), r.level(), r.args());
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Some(f) = &self.file {
            let _ = f.lock().unwrap().write_all(line.as_bytes());
        }
    }
    fn flush(&self) {}
}

fn init_logger(verbose: bool) {
    let path = Config::base_dir().join("kalkan-rs.log");
    // Log rotativo simples: recomeça se passar de 1 MB.
    let truncate = std::fs::metadata(&path).map(|m| m.len() > 1 << 20).unwrap_or(false);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(!truncate)
        .write(true)
        .truncate(truncate)
        .open(&path)
        .ok()
        .map(Mutex::new);
    let level = if verbose { log::LevelFilter::Debug } else { log::LevelFilter::Info };
    let _ = log::set_boxed_logger(Box::new(Logger { level, file }));
    log::set_max_level(level);
}

// ───────────────────────────── CLI ─────────────────────────────

const HELP: &str = "\
kalkan-rs — painel do display AIO KALKAN / GAMDIAS (320x240)

USO:
  kalkan-rs                       inicia com ícone na bandeja
  kalkan-rs --theme <nome>        inicia com o tema indicado
  kalkan-rs --no-tray             roda sem ícone na bandeja (Ctrl+C encerra)
  kalkan-rs --preview [tema] [saida.png]
                                  renderiza o tema em PNG sem usar o hardware
  kalkan-rs --record <tema> <saida.gif> [segundos]
                                  grava a animação do tema em GIF (pasta ou nome)
  kalkan-rs --autostart on|off    inicia (ou não) junto com o Windows
  kalkan-rs --list-themes         lista os temas disponíveis
  kalkan-rs --verbose             log detalhado (kalkan-rs.log)
";

#[derive(Default)]
struct Args {
    theme: Option<String>,
    no_tray: bool,
    preview: Option<(Option<String>, Option<PathBuf>)>,
    record: Option<(String, PathBuf, f32)>,
    autostart: Option<bool>,
    list: bool,
    verbose: bool,
}

fn parse_args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--theme" | "-t" => a.theme = it.next(),
            "--no-tray" => a.no_tray = true,
            "--verbose" | "-v" => a.verbose = true,
            "--list-themes" => a.list = true,
            "--preview" => {
                let mut pos = Vec::new();
                while let Some(p) = it.peek() {
                    if p.starts_with("--") {
                        break;
                    }
                    pos.push(it.next().unwrap());
                }
                let mut pos = pos.into_iter();
                a.preview = Some((pos.next(), pos.next().map(PathBuf::from)));
            }
            "--autostart" => {
                a.autostart = match it.next().as_deref() {
                    Some("on") | Some("true") | Some("1") => Some(true),
                    Some("off") | Some("false") | Some("0") => Some(false),
                    _ => {
                        eprintln!("uso: kalkan-rs --autostart on|off");
                        std::process::exit(2);
                    }
                };
            }
            "--record" => {
                let theme = it.next().unwrap_or_default();
                let out = it.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("preview.gif"));
                let secs = it.peek().and_then(|s| s.parse().ok()).inspect(|_| { it.next(); }).unwrap_or(3.0);
                a.record = Some((theme, out, secs));
            }
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => {
                eprintln!("argumento desconhecido: {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    a
}

/// Em release o binário é GUI (sem console). Se foi iniciado de um terminal,
/// reaproveita o console do pai para que `--help`/`--preview` imprimam algo.
fn attach_parent_console() {
    #[cfg(not(debug_assertions))]
    unsafe {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Garante instância única (dois processos disputando a COM3 travam o display).
fn single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    unsafe {
        let h = CreateMutexW(None, true, w!("Global\\KalkanRS-SingleInstance"));
        let exists = GetLastError() == ERROR_ALREADY_EXISTS;
        // O handle fica aberto até o fim do processo de propósito.
        std::mem::forget(h);
        !exists
    }
}

fn main() {
    attach_parent_console();
    let args = parse_args();
    init_logger(args.verbose);

    let mut cfg = Config::load();
    if let Some(t) = &args.theme {
        cfg.active_theme = t.clone();
    }

    if let Some(on) = args.autostart {
        // Grava no config (o serviço em execução aplica em ~1 s) e no registro.
        cfg.autostart = on;
        cfg.save();
        kalkan_rs::autostart::sync(on);
        match kalkan_rs::autostart::registered_command() {
            Some(c) => println!("iniciar com o Windows: ligado ({c})"),
            None => println!("iniciar com o Windows: desligado"),
        }
        return;
    }

    if args.list {
        for t in cfg.list_themes() {
            println!("{t}{}", if t == cfg.active_theme { "  (ativo)" } else { "" });
        }
        return;
    }

    let hub = Arc::new(TelemetryHub::default());
    telemetry::spawn(hub.clone(), telemetry::Options::from_config(&cfg));

    if let Some((theme, out, secs)) = args.record {
        let dir = if theme == "splash" { cfg.splash_path() } else { cfg.themes_path().join(&theme) };
        if let Err(e) = engine::record_gif(&dir, &cfg.fonts_path(), &out, hub, secs, 2) {
            eprintln!("erro: {e:#}");
            std::process::exit(1);
        }
        return;
    }

    if let Some((theme, out)) = args.preview {
        let name = theme.unwrap_or_else(|| cfg.active_theme.clone());
        let dir = if name == "splash" { cfg.splash_path() } else { cfg.themes_path().join(&name) };
        let out = out.unwrap_or_else(|| PathBuf::from(format!("preview_{name}.png")));
        // Aguarda algumas amostras de telemetria para preencher sparklines.
        if let Err(e) = engine::preview(&dir, &cfg.fonts_path(), &out, hub, Duration::from_millis(3500)) {
            eprintln!("erro: {e:#}");
            std::process::exit(1);
        }
        return;
    }

    if !single_instance() {
        log::error!("kalkan-rs já está em execução");
        return;
    }
    log::info!("kalkan-rs {} iniciando (tema '{}')", env!("CARGO_PKG_VERSION"), cfg.active_theme);
    // Mantém a entrada de inicialização coerente com o config (e com o caminho atual do .exe).
    kalkan_rs::autostart::sync(cfg.autostart);

    let frames = Arc::new(TripleBuffer::new());
    let status = Arc::new(DriverStatus::default());
    let (drv_tx, drv_rx) = unbounded::<DriverCmd>();
    let (ren_tx, ren_rx) = unbounded::<RenderCmd>();

    let driver_thread = {
        let (frames, status) = (frames.clone(), status.clone());
        let dll = cfg.dll_path.clone();
        let bright = if cfg.silent { 0 } else { cfg.brightness };
        let rot = cfg.rotation_index();
        std::thread::Builder::new()
            .name("driver".into())
            .spawn(move || lcd_comm::run_driver_thread(dll, frames, drv_rx, status, bright, rot))
            .expect("thread do driver")
    };

    let render_thread = {
        let settings = RenderSettings {
            theme_dir: cfg.theme_dir(),
            fonts_dir: cfg.fonts_path(),
            max_fps: cfg.max_fps,
            cache_bytes: cfg.anim_cache_mb as usize * 1024 * 1024,
            silent: cfg.silent,
            orientation: renderer::canvas::Orientation { flip_x: cfg.flip_horizontal, flip_y: cfg.flip_vertical },
            splash: cfg.splash.then(|| (cfg.splash_path(), Duration::from_millis(cfg.splash_ms.clamp(300, 15000) as u64))),
            music: cfg.music_settings(),
        };
        let (hub, frames) = (hub.clone(), frames.clone());
        std::thread::Builder::new()
            .name("render".into())
            .spawn(move || engine::run_render_thread(settings, hub, frames, ren_rx))
            .expect("thread de renderização")
    };

    if args.no_tray {
        log::info!("modo sem bandeja; Ctrl+C para sair");
        let _ = render_thread.join();
    } else {
        match ui::tray::run(cfg.clone(), ren_tx.clone(), drv_tx.clone(), status) {
            Ok(final_cfg) => cfg = final_cfg,
            Err(e) => {
                log::error!("system tray indisponível: {e:#}");
                let _ = render_thread.join();
            }
        }
    }

    log::info!("encerrando");
    cfg.save();
    let _ = ren_tx.send(RenderCmd::Quit);
    let _ = drv_tx.send(DriverCmd::Shutdown);
    frames.poke();
    // Dá tempo ao driver de apagar a tela; não trava a saída se o USB estiver preso.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !driver_thread.is_finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}
