//! Wrapper mínimo sobre o Performance Data Helper (PDH) do Windows.
//! Usado para clock real da CPU, zonas térmicas, I/O de disco e GPUs não-NVIDIA.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Performance::*;

const PDH_MORE_DATA: u32 = 0x8000_07D2;
const PDH_FMT_NOCAP100: PDH_FMT = PDH_FMT(0x0000_8000);

pub struct PdhQuery {
    query: isize,
}

#[derive(Clone, Copy)]
pub struct Counter(isize);

// Handles PDH são usados exclusivamente pela thread de telemetria.
unsafe impl Send for PdhQuery {}

impl PdhQuery {
    pub fn new() -> Option<Self> {
        let mut query = 0isize;
        let r = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) };
        (r == 0).then_some(Self { query })
    }

    /// Adiciona um contador pelo nome em inglês (independe do idioma do Windows).
    pub fn add(&self, path: &str) -> Option<Counter> {
        let mut c = 0isize;
        let r = unsafe { PdhAddEnglishCounterW(self.query, &HSTRING::from(path), 0, &mut c) };
        if r == 0 {
            Some(Counter(c))
        } else {
            log::debug!("PDH: contador indisponível '{path}' (0x{r:08X})");
            None
        }
    }

    pub fn collect(&self) -> bool {
        unsafe { PdhCollectQueryData(self.query) == 0 }
    }

    pub fn value(&self, c: Counter) -> Option<f64> {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        let r = unsafe {
            PdhGetFormattedCounterValue(c.0, PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100.0), None, &mut v)
        };
        (r == 0 && (v.CStatus == 0 || v.CStatus == 1)).then(|| unsafe { v.Anonymous.doubleValue })
    }

    /// Valores de um contador curinga `(*)`: lista de (instância, valor).
    pub fn array(&self, c: Counter) -> Vec<(String, f64)> {
        let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100.0);
        let mut size = 0u32;
        let mut count = 0u32;
        let r = unsafe { PdhGetFormattedCounterArrayW(c.0, fmt, &mut size, &mut count, None) };
        if r != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // Buffer alinhado em u64 para os itens PDH_FMT_COUNTERVALUE_ITEM_W.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let r = unsafe { PdhGetFormattedCounterArrayW(c.0, fmt, &mut size, &mut count, Some(items)) };
        if r != 0 {
            return Vec::new();
        }
        let slice = unsafe { std::slice::from_raw_parts(items, count as usize) };
        slice
            .iter()
            .filter(|it| it.FmtValue.CStatus == 0 || it.FmtValue.CStatus == 1)
            .map(|it| {
                let name = unsafe { it.szName.to_string().unwrap_or_default() };
                (name, unsafe { it.FmtValue.Anonymous.doubleValue })
            })
            .collect()
    }
}

impl Drop for PdhQuery {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}
