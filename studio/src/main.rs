//! Kalkan Studio — editor visual de temas.
//!
//! O canvas do editor é renderizado pelo MESMO motor que alimenta o display
//! (`kalkan_rs::renderer`), com telemetria real: o que você vê é exatamente o
//! que o painel mostrará. Salvar grava o `theme.toml`; o serviço `kalkan-rs`
//! recarrega na hora (hot-reload) e aplica mudanças do `config.toml`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kalkan_rs::config::Config;
use kalkan_rs::renderer::canvas::{rgb565_to_u32, u32_to_rgb565, Canvas};
use kalkan_rs::renderer::engine::{Theme, ThemeFile};
use kalkan_rs::renderer::widgets::WidgetDefs;
use kalkan_rs::telemetry::{self, TelemetryHub, HISTORY_KEYS};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::Response;
use tauri::State;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Ordem por tipo usada pelo motor ao empatar em `z` (fundo → frente).
const TYPES: &[&str] = &["shape", "rect", "image", "sparkline", "cores", "bar", "radial_gauge", "text"];

/// Id especial que aponta para a pasta da tela de inicialização.
const SPLASH_ID: &str = "@splash";

const NUMERIC_KEYS: &[&str] = &[
    "cpu_usage", "cpu_temp", "cpu_power", "cpu_clock", "cpu_clock_ghz", "cpu_cores", "cpu_core_0",
    "gpu_usage", "gpu_temp", "gpu_hotspot", "gpu_clock", "gpu_power", "gpu_fan_pct", "gpu_fan_rpm",
    "gpu_vram_used", "gpu_vram_total", "gpu_vram_used_gb", "gpu_vram_total_gb", "gpu_vram_pct",
    "ram_used", "ram_total", "ram_used_gb", "ram_total_gb", "ram_pct", "swap_used", "swap_pct",
    "disk_pct", "disk_used_gb", "disk_total_gb", "disk_read_mb", "disk_write_mb",
    "net_down_kb", "net_up_kb", "net_down_mb", "net_up_mb",
    "weather_temp", "weather_humidity", "weather_wind", "media_playing",
];
const TEXT_KEYS: &[&str] = &[
    "cpu_name", "gpu_name", "net_down", "net_up", "disk_read", "disk_write",
    "media_title", "media_artist", "media_album", "media_state", "media_icon", "weather_desc",
];

struct Studio {
    cfg: Config,
    theme_id: Option<String>,
    theme: Option<Theme>,
    doc: Option<ThemeFile>,
    canvas: Canvas,
    last_overlay: Instant,
}

struct AppState {
    studio: Mutex<Studio>,
    hub: Arc<TelemetryHub>,
}

// ───────────────────── documento JSON ⇄ ThemeFile ─────────────────────

/// Converte o tema para o documento do editor: lista única de camadas
/// (`type` + propriedades), na ordem de desenho.
fn to_doc(file: &ThemeFile) -> CmdResult<Value> {
    let widgets = serde_json::to_value(&file.widgets).map_err(err)?;
    let mut layers: Vec<(i64, usize, Value)> = Vec::new();
    for (ti, t) in TYPES.iter().enumerate() {
        if let Some(Value::Array(items)) = widgets.get(*t) {
            for it in items {
                let mut obj = it.as_object().cloned().unwrap_or_default();
                let z = obj.get("z").and_then(Value::as_i64).unwrap_or(0);
                obj.insert("type".into(), json!(t));
                layers.push((z, ti, Value::Object(obj)));
            }
        }
    }
    // Ordenação estável: z, depois a ordem de tipos do motor.
    layers.sort_by_key(|(z, ti, _)| (*z, *ti));
    Ok(json!({
        "meta": file.meta,
        "palette": file.palette,
        "background": file.background,
        "layers": layers.into_iter().map(|(_, _, v)| v).collect::<Vec<_>>(),
    }))
}

/// Converte o documento do editor de volta, usando `z = índice` para que a
/// ordem das camadas seja exatamente a da lista.
fn from_doc(doc: &Value, for_save: bool) -> CmdResult<ThemeFile> {
    let mut grouped: Map<String, Value> = Map::new();
    for t in TYPES {
        grouped.insert(t.to_string(), Value::Array(vec![]));
    }
    if let Some(layers) = doc.get("layers").and_then(Value::as_array) {
        for (i, l) in layers.iter().enumerate() {
            let mut obj = l.as_object().cloned().ok_or("camada inválida")?;
            let t = obj.remove("type").and_then(|v| v.as_str().map(str::to_string)).ok_or("camada sem tipo")?;
            // Campos exclusivos do editor (nome, oculto, travado) não vão para o TOML.
            let hidden = obj.remove("_hidden").and_then(|v| v.as_bool()).unwrap_or(false);
            obj.retain(|k, _| !k.starts_with('_'));
            // Camadas ocultas não aparecem no preview, mas são preservadas ao salvar.
            if hidden && !for_save {
                continue;
            }
            obj.insert("z".into(), json!(i));
            match grouped.get_mut(&t) {
                Some(Value::Array(a)) => a.push(Value::Object(obj)),
                _ => return Err(format!("tipo de camada desconhecido: {t}")),
            }
        }
    }
    let widgets: WidgetDefs = serde_json::from_value(Value::Object(grouped)).map_err(err)?;
    Ok(ThemeFile {
        meta: serde_json::from_value(doc.get("meta").cloned().unwrap_or(json!({}))).map_err(err)?,
        background: serde_json::from_value(doc.get("background").cloned().unwrap_or(json!({}))).map_err(err)?,
        palette: serde_json::from_value(doc.get("palette").cloned().unwrap_or(json!({}))).map_err(err)?,
        widgets,
    })
}

/// Serializa para TOML legível com um cabeçalho.
fn to_toml(file: &ThemeFile) -> CmdResult<String> {
    let body = toml::to_string_pretty(file).map_err(err)?;
    Ok(format!(
        "# Gerado pelo Kalkan Studio — pode ser editado à mão (hot-reload).\n# Ordem de desenho = campo `z`.\n\n{body}"
    ))
}

fn theme_dir(cfg: &Config, id: &str) -> CmdResult<PathBuf> {
    if id == SPLASH_ID {
        return Ok(cfg.splash_path());
    }
    if id.is_empty() || id.contains(['/', '\\', ':']) || id.starts_with('.') {
        return Err("nome de tema inválido".into());
    }
    Ok(cfg.themes_path().join(id))
}

fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "tema".into() } else { s }
}

// ───────────────────────────── comandos ─────────────────────────────

#[tauri::command]
fn app_info(state: State<AppState>) -> Value {
    let s = state.studio.lock().unwrap();
    json!({
        "themes_dir": s.cfg.themes_path(),
        "fonts": list_fonts(&s.cfg),
        "numeric_keys": NUMERIC_KEYS,
        "text_keys": TEXT_KEYS,
        "history_keys": HISTORY_KEYS,
        "daemon_running": daemon_running(),
    })
}

fn list_fonts(cfg: &Config) -> Vec<String> {
    let mut dirs = vec![cfg.fonts_path()];
    if let Some(w) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
    }
    let mut v: Vec<String> = dirs
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l.ends_with(".ttf") || l.ends_with(".otf")
        })
        .collect();
    v.sort_by_key(|s| s.to_ascii_lowercase());
    v.dedup();
    v
}

#[tauri::command]
fn list_themes(state: State<AppState>) -> Value {
    let s = state.studio.lock().unwrap();
    let items: Vec<Value> = s
        .cfg
        .list_themes()
        .into_iter()
        .map(|id| {
            let name = Theme::read_file(&s.cfg.themes_path().join(&id))
                .ok()
                .map(|f| f.meta.name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| id.clone());
            json!({ "id": id, "name": name, "active": id == s.cfg.active_theme })
        })
        .collect();
    let mut items = items;
    if s.cfg.splash_path().join("theme.toml").is_file() {
        items.push(json!({ "id": SPLASH_ID, "name": "⚡ Tela de inicialização", "active": false, "splash": true }));
    }
    json!(items)
}

#[tauri::command]
fn open_theme(state: State<AppState>, id: String) -> CmdResult<Value> {
    let mut s = state.studio.lock().unwrap();
    let dir = theme_dir(&s.cfg, &id)?;
    let file = Theme::read_file(&dir).map_err(err)?;
    let theme = Theme::from_file(&file, &dir, &s.cfg.fonts_path(), 64 << 20).map_err(err)?;
    let doc = to_doc(&file)?;
    s.theme = Some(theme);
    s.doc = Some(file);
    s.theme_id = Some(id);
    Ok(doc)
}

/// Aplica o documento atual ao motor (sem gravar em disco).
#[tauri::command]
fn update_doc(state: State<AppState>, doc: Value) -> CmdResult<()> {
    let file = from_doc(&doc, false)?;
    let mut s = state.studio.lock().unwrap();
    let theme = s.theme.as_mut().ok_or("nenhum tema aberto")?;
    theme.apply(&file).map_err(err)?;
    s.doc = Some(file);
    Ok(())
}

/// Renderiza um quadro. Formato binário:
/// `u32 n` + `n × (f32 z, f32 x, f32 y, f32 w, f32 h)` + `320×240 RGBA`.
#[tauri::command]
fn render(state: State<AppState>) -> CmdResult<Response> {
    let snap = state.hub.get();
    let mut guard = state.studio.lock().unwrap();
    let s = &mut *guard;
    let theme = s.theme.as_mut().ok_or("nenhum tema aberto")?;
    let now = Instant::now();
    theme.bg.advance(now);
    let dt = now.duration_since(s.last_overlay).as_secs_f32().min(0.5);
    s.last_overlay = now;
    theme.update_overlay(&snap, dt);
    theme.compose(&mut s.canvas);

    let zs: Vec<i32> = theme.widgets.iter().map(|w| w.z()).collect();
    let bounds = theme.widget_bounds();
    let mut out = Vec::with_capacity(4 + bounds.len() * 20 + 320 * 240 * 4);
    out.extend_from_slice(&(bounds.len() as u32).to_le_bytes());
    for (z, b) in zs.iter().zip(&bounds) {
        out.extend_from_slice(&(*z as f32).to_le_bytes());
        for v in b {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    // Passa pelo RGB565 para mostrar as cores exatamente como o painel exibe.
    for &p in &s.canvas.pixels {
        let c = rgb565_to_u32(u32_to_rgb565(p));
        out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]);
    }
    Ok(Response::new(out))
}

#[tauri::command]
fn save_theme(state: State<AppState>, doc: Value) -> CmdResult<String> {
    let file = from_doc(&doc, true)?;
    let s = state.studio.lock().unwrap();
    let id = s.theme_id.clone().ok_or("nenhum tema aberto")?;
    let dir = theme_dir(&s.cfg, &id)?;
    let path = dir.join("theme.toml");
    if path.is_file() {
        let _ = std::fs::copy(&path, dir.join("theme.toml.bak"));
    }
    std::fs::write(&path, to_toml(&file)?).map_err(err)?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn create_theme(state: State<AppState>, name: String, from: Option<String>) -> CmdResult<String> {
    let s = state.studio.lock().unwrap();
    let base = slug(&name);
    let mut id = base.clone();
    let mut n = 2;
    while s.cfg.themes_path().join(&id).exists() {
        id = format!("{base}_{n}");
        n += 1;
    }
    let dir = theme_dir(&s.cfg, &id)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    let mut file = match &from {
        Some(src) => {
            let sdir = theme_dir(&s.cfg, src)?;
            // Copia também imagens/vídeos/fontes do tema de origem.
            for e in std::fs::read_dir(&sdir).map_err(err)?.flatten() {
                let p = e.path();
                if p.is_file() && p.file_name().is_some_and(|f| f != "theme.toml" && f != "theme.toml.bak") {
                    let _ = std::fs::copy(&p, dir.join(e.file_name()));
                }
            }
            Theme::read_file(&sdir).map_err(err)?
        }
        None => {
            let mut f = ThemeFile::default();
            f.background.kind = "solid".into();
            f.background.color = "#0B0B14".into();
            f
        }
    };
    file.meta.name = name.trim().to_string();
    std::fs::write(dir.join("theme.toml"), to_toml(&file)?).map_err(err)?;
    Ok(id)
}

/// Copia um arquivo (imagem, vídeo, fonte) para a pasta do tema aberto.
#[tauri::command]
fn import_asset(state: State<AppState>, path: String) -> CmdResult<String> {
    let s = state.studio.lock().unwrap();
    let id = s.theme_id.clone().ok_or("nenhum tema aberto")?;
    let dir = theme_dir(&s.cfg, &id)?;
    let src = Path::new(&path);
    let name = src.file_name().ok_or("arquivo inválido")?.to_string_lossy().to_string();
    let dst = dir.join(&name);
    if src != dst {
        std::fs::copy(src, &dst).map_err(err)?;
    }
    Ok(name)
}

/// Reinicia o relógio `t` das animações (para rever entradas/fades).
#[tauri::command]
fn restart_animations(state: State<AppState>) {
    if let Some(t) = state.studio.lock().unwrap().theme.as_mut() {
        t.restart_animations();
    }
}

#[tauri::command]
fn telemetry(state: State<AppState>) -> Value {
    let snap = state.hub.get();
    let mut m = Map::new();
    for k in NUMERIC_KEYS {
        m.insert(k.to_string(), snap.metric(k).map(|v| json!((v * 100.0).round() / 100.0)).unwrap_or(Value::Null));
    }
    for k in TEXT_KEYS {
        m.insert(k.to_string(), snap.text(k).map(Value::String).unwrap_or(Value::Null));
    }
    Value::Object(m)
}

#[tauri::command]
fn get_config() -> Value {
    serde_json::to_value(Config::load()).unwrap_or_default()
}

/// Grava o config.toml; o serviço em execução aplica as mudanças em ~1 s.
#[tauri::command]
fn set_config(state: State<AppState>, cfg: Value) -> CmdResult<()> {
    let c: Config = serde_json::from_value(cfg).map_err(err)?;
    c.save();
    state.studio.lock().unwrap().cfg = c;
    Ok(())
}

#[tauri::command]
fn activate_theme(state: State<AppState>, id: String) -> CmdResult<()> {
    if id == SPLASH_ID {
        return Err("a tela de inicialização é exibida automaticamente ao ligar; não pode ser o tema ativo".into());
    }
    let mut c = Config::load();
    theme_dir(&c, &id)?;
    c.active_theme = id;
    c.save();
    state.studio.lock().unwrap().cfg = c;
    Ok(())
}

fn daemon_running() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, w!("Global\\KalkanRS-SingleInstance")) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

#[tauri::command]
fn start_daemon() -> CmdResult<bool> {
    if daemon_running() {
        return Ok(true);
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("kalkan-rs.exe")))
        .filter(|p| p.is_file())
        .ok_or("kalkan-rs.exe não encontrado ao lado do Studio")?;
    std::process::Command::new(exe).current_dir(Config::base_dir()).spawn().map_err(err)?;
    Ok(true)
}

#[tauri::command]
fn daemon_status() -> bool {
    daemon_running()
}

fn main() {
    let cfg = Config::load();
    let hub = Arc::new(TelemetryHub::default());
    telemetry::spawn(hub.clone(), cfg.disk_mount.clone(), cfg.gpu_backend.clone(), cfg.weather_lat.zip(cfg.weather_lon));

    let state = AppState {
        studio: Mutex::new(Studio {
            cfg,
            theme_id: None,
            theme: None,
            doc: None,
            canvas: Canvas::new(),
            last_overlay: Instant::now(),
        }),
        hub,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_themes,
            open_theme,
            update_doc,
            render,
            save_theme,
            create_theme,
            import_asset,
            telemetry,
            restart_animations,
            get_config,
            set_config,
            activate_theme,
            start_daemon,
            daemon_status,
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Kalkan Studio");
}
