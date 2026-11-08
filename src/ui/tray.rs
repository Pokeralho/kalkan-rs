//! Ícone na bandeja do Windows com menu de temas, brilho, rotação e modo silencioso.
//! Roda na thread principal, que mantém o loop de mensagens Win32.

use crate::config::Config;
use crate::driver::lcd_comm::DriverStatus;
use crate::driver::DriverCmd;
use crate::renderer::engine::RenderCmd;
use anyhow::Result;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostQuitMessage, SetTimer, TranslateMessage, MSG, SW_SHOWNORMAL, WM_TIMER,
};

enum Action {
    Theme(String),
    Brightness(u32),
    Fps(u32),
    Rotation(u32),
    Silent,
    Music,
    Autostart,
    FlipV,
    FlipH,
    Reload,
    Studio,
    OpenThemes,
    OpenConfig,
    Quit,
}

struct Tray {
    cfg: Config,
    render: Sender<RenderCmd>,
    driver: Sender<DriverCmd>,
    status: Arc<DriverStatus>,
    actions: HashMap<MenuId, Action>,
    theme_items: Vec<(String, CheckMenuItem)>,
    themes_menu: Submenu,
    cfg_mtime: Option<std::time::SystemTime>,
    bright_items: Vec<(u32, CheckMenuItem)>,
    fps_items: Vec<(u32, CheckMenuItem)>,
    rot_items: Vec<(u32, CheckMenuItem)>,
    silent_item: CheckMenuItem,
    music_item: CheckMenuItem,
    autostart_item: CheckMenuItem,
    flip_v: CheckMenuItem,
    flip_h: CheckMenuItem,
    icon: TrayIcon,
    last_tip: String,
}

/// Ícone 32x32 gerado em código: anel ciano/magenta sobre fundo escuro.
fn make_icon() -> Icon {
    const S: usize = 32;
    let mut rgba = vec![0u8; S * S * 4];
    for y in 0..S {
        for x in 0..S {
            let (fx, fy) = (x as f32 + 0.5 - 16.0, y as f32 + 0.5 - 16.0);
            let d = (fx * fx + fy * fy).sqrt();
            let i = (y * S + x) * 4;
            let body = (15.5 - d).clamp(0.0, 1.0);
            let ring = (2.6 - (d - 11.0).abs()).clamp(0.0, 1.0);
            let t = (fx.atan2(fy) / std::f32::consts::PI + 1.0) / 2.0;
            let (r, g, b) = (
                (20.0 + ring * (255.0 * t)) as u8,
                (20.0 + ring * (230.0 * (1.0 - t))) as u8,
                (30.0 + ring * 225.0) as u8,
            );
            rgba[i..i + 4].copy_from_slice(&[r, g, b, (body * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, S as u32, S as u32).expect("ícone")
}

impl Tray {
    fn new(cfg: Config, render: Sender<RenderCmd>, driver: Sender<DriverCmd>, status: Arc<DriverStatus>) -> Result<Self> {
        let mut actions = HashMap::new();
        let menu = Menu::new();

        // Temas
        let themes_menu = Submenu::new("Temas", true);
        let mut theme_items = Vec::new();
        for t in cfg.list_themes() {
            let it = CheckMenuItem::new(&t, true, t == cfg.active_theme, None);
            actions.insert(it.id().clone(), Action::Theme(t.clone()));
            themes_menu.append(&it)?;
            theme_items.push((t, it));
        }
        if theme_items.is_empty() {
            themes_menu.append(&MenuItem::new("(nenhum tema encontrado)", false, None))?;
        }

        // Brilho
        let bright_menu = Submenu::new("Brilho", true);
        let mut bright_items = Vec::new();
        for b in [10u32, 25, 50, 75, 100] {
            let it = CheckMenuItem::new(format!("{b}%"), true, b == cfg.brightness, None);
            actions.insert(it.id().clone(), Action::Brightness(b));
            bright_menu.append(&it)?;
            bright_items.push((b, it));
        }

        // FPS
        let fps_menu = Submenu::new("FPS máximo", true);
        let mut fps_items = Vec::new();
        for f in [10u32, 15, 20, 24, 30] {
            let label = if f == 24 { format!("{f} fps (padrão)") } else { format!("{f} fps") };
            let it = CheckMenuItem::new(label, true, f == cfg.max_fps, None);
            actions.insert(it.id().clone(), Action::Fps(f));
            fps_menu.append(&it)?;
            fps_items.push((f, it));
        }

        // Rotação
        let rot_menu = Submenu::new("Rotação", true);
        let mut rot_items = Vec::new();
        for r in [0u32, 90, 180, 270] {
            let it = CheckMenuItem::new(format!("{r}°"), true, r == cfg.rotation, None);
            actions.insert(it.id().clone(), Action::Rotation(r));
            rot_menu.append(&it)?;
            rot_items.push((r, it));
        }

        let flip_v = CheckMenuItem::new("Inverter verticalmente", true, cfg.flip_vertical, None);
        actions.insert(flip_v.id().clone(), Action::FlipV);
        let flip_h = CheckMenuItem::new("Espelhar horizontalmente", true, cfg.flip_horizontal, None);
        actions.insert(flip_h.id().clone(), Action::FlipH);
        rot_menu.append_items(&[&PredefinedMenuItem::separator(), &flip_v, &flip_h])?;
        let autostart_item = CheckMenuItem::new("Iniciar com o Windows", true, cfg.autostart, None);
        actions.insert(autostart_item.id().clone(), Action::Autostart);
        let music_item = CheckMenuItem::new("Modo música automático", true, cfg.music_mode, None);
        actions.insert(music_item.id().clone(), Action::Music);
        let silent_item = CheckMenuItem::new("Modo silencioso", true, cfg.silent, None);
        actions.insert(silent_item.id().clone(), Action::Silent);
        let studio = MenuItem::new("Abrir Kalkan Studio…", true, None);
        actions.insert(studio.id().clone(), Action::Studio);
        let reload = MenuItem::new("Recarregar tema", true, None);
        actions.insert(reload.id().clone(), Action::Reload);
        let open_themes = MenuItem::new("Abrir pasta de temas", true, None);
        actions.insert(open_themes.id().clone(), Action::OpenThemes);
        let open_cfg = MenuItem::new("Editar config.toml", true, None);
        actions.insert(open_cfg.id().clone(), Action::OpenConfig);
        let quit = MenuItem::new("Sair", true, None);
        actions.insert(quit.id().clone(), Action::Quit);

        menu.append_items(&[
            &studio,
            &PredefinedMenuItem::separator(),
            &themes_menu,
            &bright_menu,
            &fps_menu,
            &rot_menu,
            &music_item,
            &silent_item,
            &autostart_item,
            &PredefinedMenuItem::separator(),
            &reload,
            &open_themes,
            &open_cfg,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Kalkan-RS")
            .with_icon(make_icon())
            .build()?;

        Ok(Self {
            cfg_mtime: config_mtime(),
            themes_menu,
            cfg,
            render,
            driver,
            status,
            actions,
            theme_items,
            bright_items,
            fps_items,
            rot_items,
            silent_item,
            music_item,
            autostart_item,
            flip_v,
            flip_h,
            icon,
            last_tip: String::new(),
        })
    }

    fn sync_checks(&self) {
        for (t, it) in &self.theme_items {
            it.set_checked(*t == self.cfg.active_theme);
        }
        for (b, it) in &self.bright_items {
            it.set_checked(*b == self.cfg.brightness);
        }
        for (f, it) in &self.fps_items {
            it.set_checked(*f == self.cfg.max_fps);
        }
        for (r, it) in &self.rot_items {
            it.set_checked(*r == self.cfg.rotation);
        }
        self.silent_item.set_checked(self.cfg.silent);
        self.music_item.set_checked(self.cfg.music_mode);
        self.autostart_item.set_checked(self.cfg.autostart);
        self.flip_v.set_checked(self.cfg.flip_vertical);
        self.flip_h.set_checked(self.cfg.flip_horizontal);
    }

    fn update_tooltip(&mut self) {
        let conn = if self.status.connected.load(Ordering::Relaxed) { "conectado" } else { "desconectado" };
        let tip = format!(
            "Kalkan-RS — {}{}\nDisplay {conn} · brilho {}%",
            self.cfg.active_theme,
            if self.cfg.silent { " (silencioso)" } else { "" },
            self.cfg.brightness
        );
        if tip != self.last_tip {
            let _ = self.icon.set_tooltip(Some(&tip));
            self.last_tip = tip;
        }
    }

    fn open(path: &std::path::Path) {
        unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL);
        }
    }

    /// Aplica mudanças feitas no config.toml por outro processo (Kalkan Studio).
    fn poll_external_config(&mut self) {
        let m = config_mtime();
        if m == self.cfg_mtime {
            return;
        }
        self.cfg_mtime = m;
        let new = Config::load();
        let old = std::mem::replace(&mut self.cfg, new);
        let c = &self.cfg;
        if c.active_theme != old.active_theme {
            let _ = self.render.send(RenderCmd::SwitchTheme(c.theme_dir()));
        }
        if c.silent != old.silent {
            let _ = self.render.send(RenderCmd::Silent(c.silent));
        }
        if c.brightness != old.brightness || c.silent != old.silent {
            let _ = self.driver.send(DriverCmd::Brightness(if c.silent { 0 } else { c.brightness }));
        }
        if (c.music_mode, &c.music_theme, c.music_return_s) != (old.music_mode, &old.music_theme, old.music_return_s) {
            let _ = self.render.send(RenderCmd::Music(c.music_settings()));
        }
        if c.autostart != old.autostart {
            crate::autostart::sync(c.autostart);
        }
        if c.max_fps != old.max_fps {
            let _ = self.render.send(RenderCmd::MaxFps(c.max_fps));
        }
        if c.rotation != old.rotation {
            let _ = self.driver.send(DriverCmd::Rotation(c.rotation_index()));
        }
        if (c.flip_horizontal, c.flip_vertical) != (old.flip_horizontal, old.flip_vertical) {
            let o = crate::renderer::canvas::Orientation { flip_x: c.flip_horizontal, flip_y: c.flip_vertical };
            let _ = self.render.send(RenderCmd::Orientation(o));
        }
        log::info!("config.toml alterado externamente; aplicado");
        self.refresh_theme_list();
        self.sync_checks();
        self.update_tooltip();
    }

    /// Acrescenta/remove temas criados ou apagados no disco.
    fn refresh_theme_list(&mut self) {
        let list = self.cfg.list_themes();
        self.theme_items.retain(|(t, it)| {
            let keep = list.contains(t);
            if !keep {
                let _ = self.themes_menu.remove(it);
            }
            keep
        });
        for t in list {
            if !self.theme_items.iter().any(|(x, _)| *x == t) {
                let it = CheckMenuItem::new(&t, true, t == self.cfg.active_theme, None);
                self.actions.insert(it.id().clone(), Action::Theme(t.clone()));
                let _ = self.themes_menu.append(&it);
                self.theme_items.push((t, it));
            }
        }
    }

    fn launch_studio() {
        let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("kalkan-studio.exe")));
        match exe.filter(|p| p.is_file()) {
            Some(p) => {
                if let Err(e) = std::process::Command::new(&p).current_dir(Config::base_dir()).spawn() {
                    log::error!("falha ao abrir o Studio: {e}");
                }
            }
            None => log::error!("kalkan-studio.exe não encontrado ao lado do kalkan-rs.exe"),
        }
    }

    /// Processa um clique de menu. Retorna `false` para encerrar.
    fn handle(&mut self, id: &MenuId) -> bool {
        let Some(action) = self.actions.get(id) else { return true };
        match action {
            Action::Theme(t) => {
                self.cfg.active_theme = t.clone();
                let _ = self.render.send(RenderCmd::SwitchTheme(self.cfg.theme_dir()));
            }
            Action::Brightness(b) => {
                self.cfg.brightness = *b;
                if !self.cfg.silent {
                    let _ = self.driver.send(DriverCmd::Brightness(*b));
                }
            }
            Action::Fps(f) => {
                self.cfg.max_fps = *f;
                let _ = self.render.send(RenderCmd::MaxFps(*f));
            }
            Action::Rotation(r) => {
                self.cfg.rotation = *r;
                let _ = self.driver.send(DriverCmd::Rotation(self.cfg.rotation_index()));
            }
            Action::Silent => {
                self.cfg.silent = !self.cfg.silent;
                let _ = self.render.send(RenderCmd::Silent(self.cfg.silent));
                let b = if self.cfg.silent { 0 } else { self.cfg.brightness };
                let _ = self.driver.send(DriverCmd::Brightness(b));
            }
            Action::Autostart => {
                self.cfg.autostart = !self.cfg.autostart;
                crate::autostart::sync(self.cfg.autostart);
            }
            Action::Music => {
                self.cfg.music_mode = !self.cfg.music_mode;
                let _ = self.render.send(RenderCmd::Music(self.cfg.music_settings()));
            }
            Action::FlipV | Action::FlipH => {
                if matches!(action, Action::FlipV) {
                    self.cfg.flip_vertical = !self.cfg.flip_vertical;
                } else {
                    self.cfg.flip_horizontal = !self.cfg.flip_horizontal;
                }
                let o = crate::renderer::canvas::Orientation {
                    flip_x: self.cfg.flip_horizontal,
                    flip_y: self.cfg.flip_vertical,
                };
                let _ = self.render.send(RenderCmd::Orientation(o));
            }
            Action::Reload => {
                let _ = self.render.send(RenderCmd::Reload);
                self.refresh_theme_list();
            }
            Action::Studio => {
                Self::launch_studio();
                return true;
            }
            Action::OpenThemes => Self::open(&self.cfg.themes_path()),
            Action::OpenConfig => {
                self.cfg.save();
                Self::open(&Config::path());
            }
            Action::Quit => return false,
        }
        self.cfg.save();
        self.cfg_mtime = config_mtime();
        self.sync_checks();
        self.update_tooltip();
        true
    }
}

fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(Config::path()).and_then(|m| m.modified()).ok()
}

/// Bloqueia a thread principal no loop de mensagens até o usuário escolher "Sair".
pub fn run(cfg: Config, render: Sender<RenderCmd>, driver: Sender<DriverCmd>, status: Arc<DriverStatus>) -> Result<Config> {
    let mut tray = Tray::new(cfg, render, driver, status)?;
    tray.update_tooltip();
    unsafe {
        // Timer da thread para atualizar o tooltip (status de conexão).
        SetTimer(None, 0, 1000, None);
        let mut msg = MSG::default();
        'outer: while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER {
                tray.poll_external_config();
                tray.update_tooltip();
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if !tray.handle(&ev.id) {
                    PostQuitMessage(0);
                    break 'outer;
                }
            }
        }
    }
    Ok(tray.cfg)
}
