//! Coletor de telemetria rodando em thread própria a 1 Hz.

pub mod cpu;
pub mod gpu;
pub mod hwmon;
pub mod lyrics;
pub mod media;
pub mod memory;
pub mod network;
pub mod pdh;
pub mod weather;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Quantidade de amostras mantidas no histórico (1 Hz).
pub const HISTORY_LEN: usize = 120;

/// Métricas com histórico circular para sparklines.
pub const HISTORY_KEYS: &[&str] = &[
    "cpu_usage", "cpu_temp", "cpu_power", "cpu_clock", "gpu_usage", "gpu_temp", "gpu_power",
    "ram_pct", "net_down_kb", "net_up_kb", "disk_read_mb", "disk_write_mb",
];

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    // CPU
    pub cpu_name: String,
    pub cpu_usage: Option<f64>,
    pub cpu_cores: Vec<f32>,
    pub cpu_temp: Option<f64>,
    pub cpu_power: Option<f64>,
    pub cpu_clock: Option<f64>,
    // GPU
    pub gpu_name: String,
    pub gpu_temp: Option<f64>,
    pub gpu_hotspot: Option<f64>,
    pub gpu_usage: Option<f64>,
    pub gpu_vram_used: Option<f64>,
    pub gpu_vram_total: Option<f64>,
    pub gpu_fan_pct: Option<f64>,
    pub gpu_fan_rpm: Option<f64>,
    pub gpu_power: Option<f64>,
    pub gpu_clock: Option<f64>,
    // Memória (MB)
    pub ram_used: Option<f64>,
    pub ram_total: Option<f64>,
    pub swap_used: Option<f64>,
    pub swap_total: Option<f64>,
    // Disco
    pub disk_used_gb: Option<f64>,
    pub disk_total_gb: Option<f64>,
    pub disk_read: Option<f64>,  // bytes/s
    pub disk_write: Option<f64>, // bytes/s
    // Rede (bytes/s)
    pub net_down: Option<f64>,
    pub net_up: Option<f64>,
    // Mídia
    pub media_title: String,
    pub media_artist: String,
    pub media_album: String,
    pub media_playing: bool,
    pub media_active: bool,
    /// Posição (s) no instante `media_sampled`; avança sozinha enquanto toca.
    pub media_position: Option<f64>,
    pub media_duration: Option<f64>,
    pub media_sampled: Option<Instant>,
    pub media_art: Option<Arc<media::AlbumArt>>,
    pub lyrics: Option<Arc<lyrics::Lyrics>>,
    // Clima
    pub weather_temp: Option<f64>,
    pub weather_humidity: Option<f64>,
    pub weather_wind: Option<f64>,
    pub weather_desc: String,
    // Histórico
    pub history: HashMap<&'static str, VecDeque<f32>>,
}

fn pct(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b * 100.0),
        _ => None,
    }
}

/// `m:ss` ou `h:mm:ss`.
pub fn fmt_clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Formata bytes/s com unidade automática.
pub fn fmt_rate(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} GB/s", bps / (1024.0 * 1024.0 * 1024.0))
    } else if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else if bps >= 1024.0 {
        format!("{:.0} KB/s", bps / 1024.0)
    } else {
        format!("{bps:.0} B/s")
    }
}

impl Snapshot {
    /// Valor numérico de uma métrica pelo nome usado nos temas.
    pub fn metric(&self, key: &str) -> Option<f64> {
        const MB: f64 = 1024.0 * 1024.0;
        Some(match key {
            "cpu_usage" => self.cpu_usage?,
            "cpu_temp" => self.cpu_temp?,
            "cpu_power" => self.cpu_power?,
            "cpu_clock" => self.cpu_clock?,
            "cpu_clock_ghz" => self.cpu_clock? / 1000.0,
            "cpu_cores" => self.cpu_cores.len() as f64,
            "gpu_temp" => self.gpu_temp?,
            "gpu_hotspot" => self.gpu_hotspot?,
            "gpu_usage" => self.gpu_usage?,
            "gpu_vram_used" => self.gpu_vram_used?,
            "gpu_vram_total" => self.gpu_vram_total?,
            "gpu_vram_used_gb" => self.gpu_vram_used? / 1024.0,
            "gpu_vram_total_gb" => self.gpu_vram_total? / 1024.0,
            "gpu_vram_pct" => pct(self.gpu_vram_used, self.gpu_vram_total)?,
            "gpu_fan_pct" => self.gpu_fan_pct?,
            "gpu_fan_rpm" => self.gpu_fan_rpm?,
            "gpu_power" => self.gpu_power?,
            "gpu_clock" => self.gpu_clock?,
            "ram_used" => self.ram_used?,
            "ram_total" => self.ram_total?,
            "ram_used_gb" => self.ram_used? / 1024.0,
            "ram_total_gb" => self.ram_total? / 1024.0,
            "ram_pct" => pct(self.ram_used, self.ram_total)?,
            "swap_used" => self.swap_used?,
            "swap_pct" => pct(self.swap_used, self.swap_total)?,
            "disk_pct" => pct(self.disk_used_gb, self.disk_total_gb)?,
            "disk_used_gb" => self.disk_used_gb?,
            "disk_total_gb" => self.disk_total_gb?,
            "disk_read_mb" => self.disk_read? / MB,
            "disk_write_mb" => self.disk_write? / MB,
            "net_down_kb" => self.net_down? / 1024.0,
            "net_up_kb" => self.net_up? / 1024.0,
            "net_down_mb" => self.net_down? / MB,
            "net_up_mb" => self.net_up? / MB,
            "media_playing" => self.media_playing as u8 as f64,
            "media_active" => self.media_active as u8 as f64,
            "media_position" => self.media_pos()?,
            "media_duration" => self.media_duration?,
            "media_remaining" => (self.media_duration? - self.media_pos()?).max(0.0),
            "media_progress" => {
                let d = self.media_duration.filter(|d| *d > 0.0)?;
                (self.media_pos()? / d * 100.0).clamp(0.0, 100.0)
            }
            "lyrics_available" => self.lyrics.is_some() as u8 as f64,
            "album_art_available" => self.media_art.is_some() as u8 as f64,
            "weather_temp" => self.weather_temp?,
            "weather_humidity" => self.weather_humidity?,
            "weather_wind" => self.weather_wind?,
            k => {
                // cpu_core_N
                let n: usize = k.strip_prefix("cpu_core_")?.parse().ok()?;
                *self.cpu_cores.get(n)? as f64
            }
        })
    }

    /// Valores textuais (nomes, mídia, taxas com unidade automática).
    pub fn text(&self, key: &str) -> Option<String> {
        Some(match key {
            "cpu_name" => self.cpu_name.clone(),
            "gpu_name" => self.gpu_name.clone(),
            "media_title" => self.media_title.clone(),
            "media_artist" => self.media_artist.clone(),
            "media_album" => self.media_album.clone(),
            "weather_desc" if !self.weather_desc.is_empty() => self.weather_desc.clone(),
            "media_state" => match (self.media_active, self.media_playing) {
                (false, _) => "Stopped".into(),
                (true, true) => "Playing".into(),
                (true, false) => "Paused".into(),
            },
            "media_position_str" => fmt_clock(self.media_pos()?),
            "media_duration_str" => fmt_clock(self.media_duration?),
            "media_remaining_str" => format!("-{}", fmt_clock((self.media_duration? - self.media_pos()?).max(0.0))),
            "lyrics_line" => self.lyric_line(0)?,
            "lyrics_next" => self.lyric_line(1)?,
            "lyrics_prev" => self.lyric_line(-1)?,
            "media_icon" => match (self.media_active, self.media_playing) {
                (false, _) => "■".into(),
                (true, true) => "▶".into(),
                (true, false) => "❚❚".into(),
            },
            "net_down" => fmt_rate(self.net_down?),
            "net_up" => fmt_rate(self.net_up?),
            "disk_read" => fmt_rate(self.disk_read?),
            "disk_write" => fmt_rate(self.disk_write?),
            _ => return None,
        })
    }

    /// Posição atual da faixa, extrapolada desde a última leitura (suave entre amostras).
    pub fn media_pos(&self) -> Option<f64> {
        let p = self.media_position?;
        let extra = match (self.media_playing, self.media_sampled) {
            (true, Some(t)) => t.elapsed().as_secs_f64(),
            _ => 0.0,
        };
        Some(match self.media_duration {
            Some(d) => (p + extra).min(d),
            None => p + extra,
        })
    }

    /// Índice da linha de letra atual.
    pub fn lyric_index(&self) -> Option<usize> {
        self.lyrics.as_ref()?.index_at(self.media_pos()?, self.media_duration)
    }

    fn lyric_line(&self, offset: isize) -> Option<String> {
        let l = self.lyrics.as_ref()?;
        let i = self.lyric_index().map(|i| i as isize).unwrap_or(-1) + offset;
        if i < 0 {
            return Some(String::new());
        }
        Some(l.lines.get(i as usize).map(|(_, t)| t.clone()).unwrap_or_default())
    }

    pub fn history(&self, key: &str) -> Option<&VecDeque<f32>> {
        self.history.get(key)
    }

    fn push_history(&mut self) {
        for &k in HISTORY_KEYS {
            let v = self.metric(k).unwrap_or(0.0) as f32;
            let h = self.history.entry(k).or_insert_with(|| VecDeque::with_capacity(HISTORY_LEN));
            if h.len() == HISTORY_LEN {
                h.pop_front();
            }
            h.push_back(v);
        }
    }
}

/// Ponto de troca entre a thread de telemetria e o renderizador.
#[derive(Default)]
pub struct TelemetryHub {
    snap: Mutex<Arc<Snapshot>>,
    version: AtomicU64,
}

impl TelemetryHub {
    pub fn get(&self) -> Arc<Snapshot> {
        self.snap.lock().unwrap().clone()
    }
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
    fn set(&self, s: Snapshot) {
        *self.snap.lock().unwrap() = Arc::new(s);
        self.version.fetch_add(1, Ordering::Release);
    }
}

/// Opções da coleta.
#[derive(Clone, Debug)]
pub struct Options {
    pub disk_mount: String,
    pub gpu_backend: String,
    pub weather_at: Option<(f64, f64)>,
    /// Busca letras no LRCLIB quando a faixa muda.
    pub lyrics: bool,
}

impl Options {
    pub fn from_config(c: &crate::config::Config) -> Self {
        Self {
            disk_mount: c.disk_mount.clone(),
            gpu_backend: c.gpu_backend.clone(),
            weather_at: c.weather_lat.zip(c.weather_lon),
            lyrics: c.lyrics,
        }
    }
}

pub fn spawn(hub: Arc<TelemetryHub>, opts: Options) -> std::thread::JoinHandle<()> {
    let weather = opts.weather_at.map(|(lat, lon)| weather::spawn(lat, lon));
    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || run(hub, opts.disk_mount, opts.gpu_backend, weather, opts.lyrics))
        .expect("thread de telemetria")
}

fn run(hub: Arc<TelemetryHub>, disk_mount: String, gpu_backend: String, weather: Option<weather::WeatherCell>, lyrics: bool) {
    // COM (MTA) para WMI e WinRT nesta thread.
    let com = wmi::COMLibrary::new().ok();
    let (mut stat_since, mut stat_cpu) = (Instant::now(), crate::driver::thread_cpu_time());
    let pdh = pdh::PdhQuery::new();

    let mut hw = hwmon::HwMonitor::new(com);
    let mut cpu = cpu::CpuCollector::new(pdh.as_ref(), com);
    let mut gpu = gpu::GpuCollector::new(pdh.as_ref(), &gpu_backend);
    let mut mem = memory::MemoryCollector::new(pdh.as_ref(), disk_mount);
    let mut net = network::NetworkCollector::new();
    let mut media = media::MediaCollector::new(lyrics);

    let mut snap = Snapshot::default();
    let period = Duration::from_secs(1);
    let mut next = Instant::now();

    // Primeira coleta PDH serve de base para contadores de taxa.
    if let Some(p) = &pdh {
        p.collect();
    }

    loop {
        next += period;
        if let Some(p) = &pdh {
            p.collect();
        }
        let sensors = hw.poll();
        cpu.update(&mut snap, pdh.as_ref(), &sensors);
        gpu.update(&mut snap, pdh.as_ref(), &sensors);
        mem.update(&mut snap, pdh.as_ref());
        net.update(&mut snap);
        media.update(&mut snap);
        if let Some(w) = weather.as_ref().and_then(|c| c.lock().unwrap().clone()) {
            snap.weather_temp = Some(w.temp);
            snap.weather_humidity = Some(w.humidity);
            snap.weather_wind = Some(w.wind);
            snap.weather_desc = weather::describe(w.code).to_string();
        }
        snap.push_history();
        hub.set(snap.clone());
        if stat_since.elapsed() >= Duration::from_secs(10) {
            let c = crate::driver::thread_cpu_time();
            log::debug!(
                "telemetria: CPU da thread {:.2}% de um núcleo",
                (c - stat_cpu).as_secs_f64() / stat_since.elapsed().as_secs_f64() * 100.0
            );
            (stat_since, stat_cpu) = (Instant::now(), c);
        }

        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}
