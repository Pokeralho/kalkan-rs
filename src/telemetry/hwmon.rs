//! Sensores de baixo nível via WMI do LibreHardwareMonitor / OpenHardwareMonitor.
//!
//! O Windows não expõe Tdie/Tctl, watts da CPU nem hotspot da GPU por APIs
//! públicas sem driver de kernel. Se o LibreHardwareMonitor estiver rodando
//! (com o servidor WMI ativo, que é o padrão), estes valores são lidos dele.

use serde::Deserialize;
use std::time::{Duration, Instant};
use wmi::{COMLibrary, WMIConnection};

#[derive(Debug, Default, Clone)]
pub struct HwSensors {
    pub cpu_temp: Option<f64>,
    pub cpu_power: Option<f64>,
    pub cpu_clock: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_hotspot: Option<f64>,
    pub gpu_fan_rpm: Option<f64>,
    pub gpu_power: Option<f64>,
    pub gpu_usage: Option<f64>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct Sensor {
    name: String,
    sensor_type: String,
    identifier: String,
    value: f32,
}

pub struct HwMonitor {
    com: Option<COMLibrary>,
    conn: Option<WMIConnection>,
    last_probe: Option<Instant>,
}

const NAMESPACES: &[&str] = &["root\\LibreHardwareMonitor", "root\\OpenHardwareMonitor"];
const REPROBE: Duration = Duration::from_secs(30);

impl HwMonitor {
    pub fn new(com: Option<COMLibrary>) -> Self {
        Self { com, conn: None, last_probe: None }
    }

    fn probe(&mut self) {
        let Some(com) = self.com else { return };
        if self.last_probe.is_some_and(|t| t.elapsed() < REPROBE) {
            return;
        }
        self.last_probe = Some(Instant::now());
        for ns in NAMESPACES {
            if let Ok(conn) = WMIConnection::with_namespace_path(ns, com) {
                // O namespace pode existir mas estar vazio (app fechado).
                if conn.raw_query::<Sensor>("SELECT Name, SensorType, Identifier, Value FROM Sensor").is_ok_and(|v| !v.is_empty()) {
                    log::info!("sensores de hardware via {ns}");
                    self.conn = Some(conn);
                    return;
                }
            }
        }
    }

    pub fn poll(&mut self) -> HwSensors {
        if self.conn.is_none() {
            self.probe();
        }
        let Some(conn) = &self.conn else { return HwSensors::default() };
        let sensors: Vec<Sensor> = match conn.raw_query("SELECT Name, SensorType, Identifier, Value FROM Sensor") {
            Ok(v) => v,
            Err(_) => {
                self.conn = None;
                return HwSensors::default();
            }
        };

        let mut out = HwSensors::default();
        let mut cpu_temp_rank = 0u8;
        let mut clock_sum = 0.0f64;
        let mut clock_n = 0u32;

        for s in &sensors {
            let id = s.identifier.to_ascii_lowercase();
            let name = s.name.to_ascii_lowercase();
            let v = s.value as f64;
            let is_cpu = id.contains("cpu");
            let is_gpu = id.contains("gpu");
            match s.sensor_type.as_str() {
                "Temperature" if is_cpu => {
                    // Preferência: Tctl/Tdie > Package > Core Max > qualquer
                    let rank = if name.contains("tdie") || name.contains("tctl") {
                        4
                    } else if name.contains("package") {
                        3
                    } else if name.contains("max") {
                        2
                    } else {
                        1
                    };
                    if rank > cpu_temp_rank {
                        cpu_temp_rank = rank;
                        out.cpu_temp = Some(v);
                    }
                }
                "Temperature" if is_gpu => {
                    if name.contains("hot spot") || name.contains("hotspot") {
                        out.gpu_hotspot = Some(v);
                    } else if name == "gpu core" || out.gpu_temp.is_none() {
                        out.gpu_temp = Some(v);
                    }
                }
                "Power" if is_cpu => {
                    if name.contains("package") || out.cpu_power.is_none() {
                        out.cpu_power = Some(v);
                    }
                }
                "Power" if is_gpu => {
                    if name.contains("package") || name.contains("total") || out.gpu_power.is_none() {
                        out.gpu_power = Some(v);
                    }
                }
                "Clock" if is_cpu && name.starts_with("core") => {
                    clock_sum += v;
                    clock_n += 1;
                }
                "Fan" if is_gpu => {
                    out.gpu_fan_rpm = Some(out.gpu_fan_rpm.unwrap_or(0.0).max(v));
                }
                "Load" if is_gpu && name == "gpu core" => out.gpu_usage = Some(v),
                _ => {}
            }
        }
        if clock_n > 0 {
            out.cpu_clock = Some(clock_sum / clock_n as f64);
        }
        out
    }
}
