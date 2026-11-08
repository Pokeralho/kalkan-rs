//! GPU – três backends:
//!
//! * `d3dkmt` (padrão): mesma fonte do Gerenciador de Tarefas. Funciona para
//!   NVIDIA/AMD/Intel sem carregar bibliotecas do fabricante: temperatura,
//!   RPM do fan, clock (D3DKMT), VRAM (DXGI/PDH) e uso (PDH "GPU Engine").
//! * `nvml`: NVIDIA Management Library – adiciona watts e fan % reais, mas
//!   custa ~19 MB de RAM privada (só a `nvml.dll`).
//! * `pdh`: último recurso (apenas uso e VRAM).
//!
//! Hotspot/watts/RPM ausentes são complementados pelo LibreHardwareMonitor.

use super::hwmon::HwSensors;
use super::pdh::{Counter, PdhQuery};
use super::Snapshot;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use windows::Wdk::Graphics::Direct3D::*;
use windows::Win32::Graphics::Dxgi::*;

struct KmtAdapter {
    handle: u32,
    total_mb: f64,
    supports_temp: bool,
}

enum Backend {
    Nvml(Nvml),
    Kmt(KmtAdapter),
    Pdh,
}

pub struct GpuCollector {
    backend: Backend,
    name: String,
    /// Fragmento `luid_0x…_0x…` usado para filtrar os contadores PDH do adaptador.
    luid_tag: Option<String>,
    engine: Option<Counter>,
    mem: Option<Counter>,
}

/// Adaptador físico principal (maior VRAM dedicada, ignorando adaptadores de software).
fn primary_adapter() -> Option<DXGI_ADAPTER_DESC1> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut best: Option<DXGI_ADAPTER_DESC1> = None;
        let mut i = 0;
        while let Ok(a) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(d) = a.GetDesc1() else { continue };
            if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            if best.as_ref().is_none_or(|b| d.DedicatedVideoMemory > b.DedicatedVideoMemory) {
                best = Some(d);
            }
        }
        best
    }
}

fn kmt_query<T>(handle: u32, kind: KMTQUERYADAPTERINFOTYPE, mut data: T) -> Option<T> {
    let mut q = D3DKMT_QUERYADAPTERINFO {
        hAdapter: handle,
        Type: kind,
        pPrivateDriverData: &mut data as *mut T as *mut _,
        PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
    };
    let st = unsafe { D3DKMTQueryAdapterInfo(&mut q) };
    st.is_ok().then_some(data)
}

impl GpuCollector {
    pub fn new(pdh: Option<&PdhQuery>, backend_pref: &str) -> Self {
        let adapter = primary_adapter();
        let mut name = adapter
            .as_ref()
            .map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string())
            .unwrap_or_default();
        let luid_tag = adapter.as_ref().map(|d| {
            format!("luid_0x{:08X}_0x{:08X}", d.AdapterLuid.HighPart as u32, d.AdapterLuid.LowPart)
        });

        let try_nvml = || -> Option<Backend> {
            let nvml = Nvml::init().ok()?;
            (nvml.device_count().ok()? > 0).then_some(Backend::Nvml(nvml))
        };
        let try_kmt = || -> Option<Backend> {
            let d = adapter.as_ref()?;
            let mut open = D3DKMT_OPENADAPTERFROMLUID { AdapterLuid: d.AdapterLuid, hAdapter: 0 };
            unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }.ok().ok()?;
            let perf = kmt_query(open.hAdapter, KMTQAITYPE_ADAPTERPERFDATA, D3DKMT_ADAPTER_PERFDATA::default());
            Some(Backend::Kmt(KmtAdapter {
                handle: open.hAdapter,
                total_mb: d.DedicatedVideoMemory as f64 / 1048576.0,
                supports_temp: perf.is_some_and(|p| p.Temperature > 0),
            }))
        };

        let backend = match backend_pref {
            "nvml" => try_nvml().or_else(try_kmt),
            "pdh" => None,
            // auto: D3DKMT; se o driver não reporta temperatura, tenta NVML.
            _ => match try_kmt() {
                Some(Backend::Kmt(k)) if !k.supports_temp => try_nvml().or(Some(Backend::Kmt(k))),
                other => other.or_else(try_nvml),
            },
        }
        .unwrap_or(Backend::Pdh);

        if let Backend::Nvml(n) = &backend {
            if let Ok(d) = n.device_by_index(0) {
                name = d.name().unwrap_or(name);
            }
        }
        log::info!(
            "GPU via {}: {name}",
            match backend {
                Backend::Nvml(_) => "NVML",
                Backend::Kmt(_) => "D3DKMT",
                Backend::Pdh => "PDH",
            }
        );
        Self {
            engine: pdh.and_then(|p| p.add(r"\GPU Engine(*)\Utilization Percentage")),
            mem: pdh.and_then(|p| p.add(r"\GPU Adapter Memory(*)\Dedicated Usage")),
            backend,
            name,
            luid_tag,
        }
    }

    fn matches(&self, inst: &str) -> bool {
        self.luid_tag.as_deref().is_none_or(|t| inst.contains(t))
    }

    /// Uso 3D do adaptador principal (soma dos processos no motor 3D).
    fn pdh_usage(&self, p: &PdhQuery) -> Option<f64> {
        let c = self.engine?;
        let mut per_luid: HashMap<String, f64> = HashMap::new();
        for (inst, v) in p.array(c) {
            if !inst.ends_with("engtype_3D") || !self.matches(&inst) {
                continue;
            }
            let luid = inst.split("_luid_").nth(1).and_then(|r| r.split("_phys").next()).unwrap_or("").to_string();
            *per_luid.entry(luid).or_default() += v;
        }
        per_luid.values().copied().reduce(f64::max).map(|v| v.min(100.0))
    }

    fn pdh_vram(&self, p: &PdhQuery) -> Option<f64> {
        p.array(self.mem?)
            .into_iter()
            .filter(|(i, _)| self.matches(i))
            .map(|(_, v)| v / 1048576.0)
            .reduce(f64::max)
    }

    pub fn update(&mut self, s: &mut Snapshot, pdh: Option<&PdhQuery>, hw: &HwSensors) {
        if s.gpu_name.is_empty() {
            s.gpu_name = self.name.clone();
        }
        match &self.backend {
            Backend::Nvml(nvml) => {
                if let Ok(d) = nvml.device_by_index(0) {
                    s.gpu_temp = d.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f64);
                    s.gpu_usage = d.utilization_rates().ok().map(|u| u.gpu as f64);
                    if let Ok(m) = d.memory_info() {
                        s.gpu_vram_used = Some(m.used as f64 / 1048576.0);
                        s.gpu_vram_total = Some(m.total as f64 / 1048576.0);
                    }
                    s.gpu_fan_pct = d.fan_speed(0).ok().map(|f| f as f64);
                    s.gpu_power = d.power_usage().ok().map(|mw| mw as f64 / 1000.0);
                    s.gpu_clock = d.clock_info(Clock::Graphics).ok().map(|c| c as f64);
                }
            }
            Backend::Kmt(k) => {
                if let Some(p) = kmt_query(k.handle, KMTQAITYPE_ADAPTERPERFDATA, D3DKMT_ADAPTER_PERFDATA::default()) {
                    s.gpu_temp = (p.Temperature > 0).then(|| p.Temperature as f64 / 10.0);
                    s.gpu_fan_rpm = (p.FanRPM > 0).then_some(p.FanRPM as f64);
                }
                if let Some(n) = kmt_query(k.handle, KMTQAITYPE_NODEPERFDATA, D3DKMT_NODE_PERFDATA::default()) {
                    s.gpu_clock = (n.Frequency > 0).then(|| n.Frequency as f64 / 1e6);
                }
                s.gpu_vram_total = Some(k.total_mb);
                if let Some(p) = pdh {
                    s.gpu_vram_used = self.pdh_vram(p);
                    s.gpu_usage = self.pdh_usage(p);
                }
            }
            Backend::Pdh => {
                if let Some(p) = pdh {
                    s.gpu_usage = self.pdh_usage(p);
                    s.gpu_vram_used = self.pdh_vram(p);
                }
            }
        }
        // Complementos do LibreHardwareMonitor
        s.gpu_hotspot = hw.gpu_hotspot;
        s.gpu_fan_rpm = s.gpu_fan_rpm.or(hw.gpu_fan_rpm);
        s.gpu_temp = s.gpu_temp.or(hw.gpu_temp);
        s.gpu_power = s.gpu_power.or(hw.gpu_power);
        s.gpu_usage = s.gpu_usage.or(hw.gpu_usage);
    }
}
