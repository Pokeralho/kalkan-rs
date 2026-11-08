//! Configuração global persistida em `config.toml` ao lado do executável.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Limite do hardware (30 fps).
pub const MAX_FPS_LIMIT: u32 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Nome da pasta do tema ativo dentro de `themes_dir`.
    pub active_theme: String,
    /// Brilho do painel (0..=100).
    pub brightness: u32,
    /// Rotação do painel: 0, 90, 180 ou 270 graus.
    pub rotation: u32,
    /// Espelha a imagem verticalmente (painel montado de cabeça para baixo).
    pub flip_vertical: bool,
    /// Espelha a imagem horizontalmente.
    pub flip_horizontal: bool,
    /// Modo silencioso: tela preta, brilho 0, renderização pausada.
    pub silent: bool,
    /// FPS máximo enviado ao display (1..=30, padrão 24). O custo de CPU do
    /// envio é proporcional aos quadros transferidos pelo USB.
    pub max_fps: u32,
    /// Pasta dos temas (relativa ao executável ou absoluta).
    pub themes_dir: PathBuf,
    /// Pasta de fontes compartilhadas.
    pub fonts_dir: PathBuf,
    /// Caminho explícito da LcdComm.dll (opcional).
    pub dll_path: Option<PathBuf>,
    /// Orçamento (MB) para cache de frames de GIF/WebP/APNG na RAM.
    /// Animações maiores são decodificadas em streaming.
    pub anim_cache_mb: u32,
    /// Ponto de montagem do disco monitorado.
    pub disk_mount: String,
    /// Backend de GPU: "auto" (D3DKMT, leve), "nvml" (+watts/fan%, ~19 MB) ou "pdh".
    pub gpu_backend: String,
    /// Tela de inicialização exibida ao ligar (é um tema comum, editável no Studio).
    pub splash: bool,
    /// Duração da tela de inicialização em milissegundos.
    pub splash_ms: u32,
    /// Pasta do tema da tela de inicialização.
    pub splash_dir: PathBuf,
    /// Coordenadas para o clima (Open-Meteo). Ausentes = clima desativado.
    pub weather_lat: Option<f64>,
    pub weather_lon: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_theme: "cyberpunk".into(),
            brightness: 80,
            rotation: 0,
            flip_vertical: true,
            flip_horizontal: true,
            silent: false,
            max_fps: 24,
            themes_dir: PathBuf::from("assets/themes"),
            fonts_dir: PathBuf::from("assets/fonts"),
            dll_path: None,
            anim_cache_mb: 12,
            disk_mount: "C:\\".into(),
            gpu_backend: "auto".into(),
            splash: true,
            splash_ms: 2600,
            splash_dir: PathBuf::from("assets/splash"),
            weather_lat: None,
            weather_lon: None,
        }
    }
}

impl Config {
    /// Diretório base: pasta do executável, ou o diretório atual quando os
    /// assets estiverem lá (útil durante `cargo run`).
    pub fn base_dir() -> PathBuf {
        let cwd = std::env::current_dir().unwrap_or_default();
        if cwd.join("assets").is_dir() {
            return cwd;
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or(cwd)
    }

    pub fn path() -> PathBuf {
        Self::base_dir().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            // Tolera BOM UTF-8 (alguns editores e o PowerShell 5.1 o adicionam).
            Ok(s) => match toml::from_str::<Config>(s.trim_start_matches('\u{feff}')) {
                Ok(mut c) => {
                    c.sanitize();
                    c
                }
                Err(e) => {
                    log::warn!("config.toml inválido ({e}); usando padrões");
                    Self::default()
                }
            },
            Err(_) => {
                let c = Self::default();
                c.save();
                c
            }
        }
    }

    pub fn save(&self) {
        match toml::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = std::fs::write(Self::path(), s) {
                    log::warn!("falha ao salvar config: {e}");
                }
            }
            Err(e) => log::warn!("falha ao serializar config: {e}"),
        }
    }

    fn sanitize(&mut self) {
        self.brightness = self.brightness.min(100);
        self.rotation = (self.rotation / 90 % 4) * 90;
        self.max_fps = self.max_fps.clamp(1, MAX_FPS_LIMIT);
    }

    fn resolve(p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            Self::base_dir().join(p)
        }
    }

    pub fn themes_path(&self) -> PathBuf {
        Self::resolve(&self.themes_dir)
    }

    pub fn fonts_path(&self) -> PathBuf {
        Self::resolve(&self.fonts_dir)
    }

    pub fn splash_path(&self) -> PathBuf {
        Self::resolve(&self.splash_dir)
    }

    pub fn theme_dir(&self) -> PathBuf {
        self.themes_path().join(&self.active_theme)
    }

    /// Lista as pastas de tema que possuem `theme.toml`.
    pub fn list_themes(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.themes_path())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("theme.toml").is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        v.sort();
        v
    }

    /// Índice de rotação esperado pela DLL (0..=3).
    pub fn rotation_index(&self) -> u32 {
        self.rotation / 90 % 4
    }
}
