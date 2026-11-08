//! Wrapper seguro para `LcdComm.dll` (GAMDIAS / KALKAN) carregada em runtime.
//!
//! Assinaturas (x64, `stdcall` == ABI "system"):
//! ```c
//! void     SetUsbID(const uint32_t* pUsbList, uint32_t length);
//! void     UsbDeviceEvent(void);
//! void     GetDevices(uint8_t* pDeviceList);              // 128 bytes
//! bool     PlayOneRGBFrame(uint8_t index, const uint8_t* pRGB, uint32_t size);
//! uint32_t SetDeviceBrightness(uint32_t index, uint32_t brightness);
//! uint32_t SetDeviceDegree(uint32_t index, uint32_t degree);
//! ```

use super::{blank_frame, DriverCmd, Frame565, TripleBuffer, FRAME_BYTES};
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::Receiver;
use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// VID 0x1B80 / PID 0xB550 combinados.
pub const USB_ID: u32 = 0x1B80_B550;
pub const DEFAULT_DLL: &str = r"C:\Program Files\KALKAN\LcdComm.dll";

type FnSetUsbId = unsafe extern "system" fn(*const u32, u32);
type FnVoid = unsafe extern "system" fn();
type FnGetDevices = unsafe extern "system" fn(*mut u8);
// `bool` em C++/MSVC ocupa 1 byte; lido como u8 para nunca gerar UB.
type FnPlayFrame = unsafe extern "system" fn(u8, *const u8, u32) -> u8;
type FnSetU32 = unsafe extern "system" fn(u32, u32) -> u32;

pub struct LcdComm {
    set_usb_id: FnSetUsbId,
    usb_device_event: FnVoid,
    get_devices: FnGetDevices,
    play_one_rgb_frame: FnPlayFrame,
    set_device_brightness: FnSetU32,
    set_device_degree: FnSetU32,
    // Mantém a DLL carregada enquanto os ponteiros acima existirem.
    _lib: Library,
    pub path: PathBuf,
}

// Os ponteiros de função são apenas endereços dentro da DLL carregada;
// o objeto inteiro vive e é usado numa única thread (a do driver).
unsafe impl Send for LcdComm {}

impl LcdComm {
    /// Procura a DLL: caminho explícito → caminho padrão → pasta do executável → diretório atual.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(p) = explicit {
            candidates.push(p.to_path_buf());
        }
        candidates.push(PathBuf::from(DEFAULT_DLL));
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
            candidates.push(dir.join("LcdComm.dll"));
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("LcdComm.dll"));
        }

        let mut last_err = anyhow!("LcdComm.dll não encontrada");
        for path in candidates {
            if !path.is_file() {
                continue;
            }
            match unsafe { Self::load_from(&path) } {
                Ok(l) => return Ok(l),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    unsafe fn load_from(path: &Path) -> Result<Self> {
        // A DLL pode depender de outras DLLs da mesma pasta: carregamos com o
        // diretório dela na ordem de busca.
        let lib = {
            use libloading::os::windows::{Library as WinLib, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};
            WinLib::load_with_flags(path, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)
                .map(Library::from)
                .with_context(|| format!("carregando {}", path.display()))?
        };

        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let s: Symbol<$ty> = lib
                    .get(concat!($name, "\0").as_bytes())
                    .with_context(|| format!("símbolo {} ausente", $name))?;
                *s
            }};
        }

        Ok(Self {
            set_usb_id: sym!("SetUsbID", FnSetUsbId),
            usb_device_event: sym!("UsbDeviceEvent", FnVoid),
            get_devices: sym!("GetDevices", FnGetDevices),
            play_one_rgb_frame: sym!("PlayOneRGBFrame", FnPlayFrame),
            set_device_brightness: sym!("SetDeviceBrightness", FnSetU32),
            set_device_degree: sym!("SetDeviceDegree", FnSetU32),
            _lib: lib,
            path: path.to_path_buf(),
        })
    }

    /// Registra o VID/PID, abre o dispositivo e verifica se ele respondeu.
    pub fn connect(&self) -> bool {
        let ids = [USB_ID];
        let mut list = [0u8; 128];
        unsafe {
            (self.set_usb_id)(ids.as_ptr(), ids.len() as u32);
            (self.usb_device_event)();
            (self.get_devices)(list.as_mut_ptr());
        }
        list[0] > 0
    }

    pub fn play_frame(&self, frame: &[u8; FRAME_BYTES]) -> bool {
        unsafe { (self.play_one_rgb_frame)(0, frame.as_ptr(), FRAME_BYTES as u32) != 0 }
    }

    pub fn set_brightness(&self, value: u32) -> u32 {
        unsafe { (self.set_device_brightness)(0, value.min(100)) }
    }

    /// `degree`: 0=0°, 1=90°, 2=180°, 3=270°
    pub fn set_degree(&self, degree: u32) -> u32 {
        unsafe { (self.set_device_degree)(0, degree % 4) }
    }
}

/// Status compartilhado com a UI.
#[derive(Default)]
pub struct DriverStatus {
    pub connected: AtomicBool,
}

/// Loop da thread do driver: consome frames do triple buffer, aplica comandos,
/// reenvia keep-alive e reconecta automaticamente após desconexão do cabo.
pub fn run_driver_thread(
    dll_path: Option<PathBuf>,
    frames: Arc<TripleBuffer>,
    cmds: Receiver<DriverCmd>,
    status: Arc<DriverStatus>,
    mut brightness: u32,
    mut rotation: u32,
) {
    let lcd = match LcdComm::load(dll_path.as_deref()) {
        Ok(l) => {
            log::info!("LcdComm.dll carregada de {}", l.path.display());
            Some(l)
        }
        Err(e) => {
            log::error!("driver indisponível: {e:#}. Rodando sem hardware.");
            None
        }
    };

    let mut front: Frame565 = blank_frame();
    let mut have_frame = false;
    let mut connected = false;
    let mut failures = 0u32;
    let mut last_attempt = Instant::now() - Duration::from_secs(60);
    let mut last_send = Instant::now();
    let (mut stat_frames, mut stat_time, mut stat_since) = (0u32, Duration::ZERO, Instant::now());
    let mut stat_cpu = super::thread_cpu_split();
    let mut last_sent: Frame565 = blank_frame();
    let mut skipped = 0u32;
    const KEEPALIVE: Duration = Duration::from_millis(1000);
    const RETRY: Duration = Duration::from_secs(2);

    loop {
        // Comandos pendentes
        while let Ok(cmd) = cmds.try_recv() {
            match cmd {
                DriverCmd::Shutdown => {
                    if let (Some(l), true) = (&lcd, connected) {
                        // Deixa a tela preta ao sair.
                        l.play_frame(&blank_frame());
                    }
                    return;
                }
                DriverCmd::Brightness(b) => {
                    brightness = b;
                    if let (Some(l), true) = (&lcd, connected) {
                        l.set_brightness(b);
                    }
                }
                DriverCmd::Rotation(r) => {
                    rotation = r;
                    if let (Some(l), true) = (&lcd, connected) {
                        l.set_degree(r);
                    }
                }
            }
        }

        let Some(l) = &lcd else {
            // Sem hardware: apenas drena os frames para não acumular.
            frames.take(&mut front, Duration::from_millis(500));
            continue;
        };

        if !connected {
            if last_attempt.elapsed() >= RETRY {
                last_attempt = Instant::now();
                if l.connect() {
                    log::info!("display conectado");
                    connected = true;
                    failures = 0;
                    l.set_brightness(brightness);
                    l.set_degree(rotation);
                    status.connected.store(true, Ordering::Relaxed);
                    if have_frame {
                        l.play_frame(&front);
                    }
                } else {
                    log::debug!("display não encontrado; nova tentativa em {RETRY:?}");
                }
            }
            if !connected {
                // Mantém o frame mais recente para enviar assim que reconectar.
                if frames.take(&mut front, Duration::from_millis(250)) {
                    have_frame = true;
                }
                continue;
            }
        }

        let wait = KEEPALIVE.saturating_sub(last_send.elapsed());
        let mut fresh = frames.take(&mut front, wait);
        // Quadro idêntico ao último enviado: não gasta USB (o custo de CPU do
        // envio é quase todo do driver usbser.sys, proporcional aos bytes).
        if fresh && have_frame && front[..] == last_sent[..] {
            fresh = false;
            skipped += 1;
        }
        have_frame |= fresh;
        if !(fresh || (have_frame && last_send.elapsed() >= KEEPALIVE)) {
            continue;
        }

        last_send = Instant::now();
        let ok = l.play_frame(&front);
        last_sent.copy_from_slice(&front[..]);
        stat_time += last_send.elapsed();
        stat_frames += 1;
        if stat_since.elapsed() >= Duration::from_secs(10) {
            let cpu = super::thread_cpu_split();
            let el = stat_since.elapsed().as_secs_f64();
            log::debug!(
                "driver: {:.1} fps ({skipped} repetidos ignorados), envio médio {:?}, CPU da thread: kernel {:.2}% + usuário {:.2}% de um núcleo",
                stat_frames as f64 / el,
                stat_time / stat_frames.max(1),
                (cpu.0 - stat_cpu.0).as_secs_f64() / el * 100.0,
                (cpu.1 - stat_cpu.1).as_secs_f64() / el * 100.0
            );
            stat_cpu = cpu;
            skipped = 0;
            (stat_frames, stat_time, stat_since) = (0, Duration::ZERO, Instant::now());
        }
        if ok {
            failures = 0;
        } else {
            failures += 1;
            if failures >= 3 {
                log::warn!("display parou de responder; tentando reconectar");
                connected = false;
                status.connected.store(false, Ordering::Relaxed);
                last_attempt = Instant::now() - RETRY;
            }
        }
    }
}
