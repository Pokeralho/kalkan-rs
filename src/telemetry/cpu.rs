//! CPU: uso total/por núcleo (sysinfo), clock efetivo (PDH), temperatura e watts.

use super::hwmon::HwSensors;
use super::pdh::{Counter, PdhQuery};
use super::Snapshot;
use serde::Deserialize;
use sysinfo::{CpuRefreshKind, RefreshKind, System};
use wmi::{COMLibrary, WMIConnection};

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
#[serde(rename_all = "PascalCase")]
struct AcpiZone {
    current_temperature: u32,
}

pub struct CpuCollector {
    sys: System,
    perf_pct: Option<Counter>,
    base_freq: Option<Counter>,
    thermal_zones: Option<Counter>,
    acpi: Option<WMIConnection>,
}

impl CpuCollector {
    pub fn new(pdh: Option<&PdhQuery>, com: Option<COMLibrary>) -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::new().with_cpu(CpuRefreshKind::new().with_cpu_usage()),
        );
        let (perf_pct, base_freq, thermal_zones) = match pdh {
            Some(p) => (
                p.add(r"\Processor Information(_Total)\% Processor Performance"),
                p.add(r"\Processor Information(_Total)\Processor Frequency"),
                p.add(r"\Thermal Zone Information(*)\High Precision Temperature"),
            ),
            None => (None, None, None),
        };
        // MSAcpi exige privilégios de administrador; é só mais um fallback.
        let acpi = com.and_then(|c| WMIConnection::with_namespace_path("root\\WMI", c).ok());
        Self { sys, perf_pct, base_freq, thermal_zones, acpi }
    }

    pub fn update(&mut self, s: &mut Snapshot, pdh: Option<&PdhQuery>, hw: &HwSensors) {
        self.sys.refresh_cpu_usage();
        let cpus = self.sys.cpus();
        if s.cpu_name.is_empty() {
            s.cpu_name = cpus.first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        }
        s.cpu_cores.clear();
        s.cpu_cores.extend(cpus.iter().map(|c| c.cpu_usage()));
        s.cpu_usage = Some(self.sys.global_cpu_usage() as f64);

        // Clock efetivo = frequência base × % de desempenho (inclui turbo).
        let pdh_clock = pdh.and_then(|p| {
            let perf = p.value(self.perf_pct?)?;
            let base = p.value(self.base_freq?)?;
            (base > 0.0).then_some(base * perf / 100.0)
        });
        s.cpu_clock = hw
            .cpu_clock
            .or(pdh_clock)
            .or_else(|| cpus.first().map(|c| c.frequency() as f64));

        s.cpu_power = hw.cpu_power;

        s.cpu_temp = hw
            .cpu_temp
            .or_else(|| {
                // Décimos de Kelvin → °C, maior zona térmica.
                let p = pdh?;
                p.array(self.thermal_zones?)
                    .into_iter()
                    .map(|(_, v)| v / 10.0 - 273.15)
                    .filter(|t| *t > 0.0 && *t < 150.0)
                    .reduce(f64::max)
            })
            .or_else(|| {
                let conn = self.acpi.as_ref()?;
                let zones: Vec<AcpiZone> = match conn.query() {
                    Ok(z) => z,
                    Err(_) => {
                        self.acpi = None; // sem privilégio: não tenta de novo
                        return None;
                    }
                };
                zones
                    .iter()
                    .map(|z| z.current_temperature as f64 / 10.0 - 273.15)
                    .filter(|t| *t > 0.0 && *t < 150.0)
                    .reduce(f64::max)
            });
    }
}
