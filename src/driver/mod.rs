//! Camada de comunicação com o display.

pub mod lcd_comm;

use std::sync::{Condvar, Mutex};
use std::time::Duration;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
pub const PIXELS: usize = WIDTH * HEIGHT;
/// 320 * 240 * 2 bytes (RGB565 big-endian).
pub const FRAME_BYTES: usize = PIXELS * 2;

pub type Frame565 = Box<[u8; FRAME_BYTES]>;

pub fn blank_frame() -> Frame565 {
    vec![0u8; FRAME_BYTES]
        .into_boxed_slice()
        .try_into()
        .expect("tamanho de frame")
}

/// Triple buffer: o renderizador publica frames sem nunca bloquear esperando
/// o USB, e o driver sempre consome o frame mais recente.
///
/// * `back`  – pertence ao produtor (renderizador)
/// * `mid`   – frame pronto mais recente (troca sob mutex, apenas swap de ponteiro)
/// * `front` – pertence ao consumidor (thread do driver)
pub struct TripleBuffer {
    mid: Mutex<(Frame565, bool)>,
    cv: Condvar,
}

impl TripleBuffer {
    pub fn new() -> Self {
        Self {
            mid: Mutex::new((blank_frame(), false)),
            cv: Condvar::new(),
        }
    }

    /// Publica `back` e devolve um buffer livre para o próximo frame.
    pub fn publish(&self, back: &mut Frame565) {
        let mut g = self.mid.lock().unwrap();
        std::mem::swap(&mut g.0, back);
        g.1 = true;
        drop(g);
        self.cv.notify_one();
    }

    /// Troca `front` pelo frame mais recente se houver um novo.
    /// Espera até `timeout`. Retorna `true` se um novo frame chegou.
    pub fn take(&self, front: &mut Frame565, timeout: Duration) -> bool {
        let g = self.mid.lock().unwrap();
        let (mut g, _) = self
            .cv
            .wait_timeout_while(g, timeout, |s| !s.1)
            .unwrap();
        if g.1 {
            std::mem::swap(&mut g.0, front);
            g.1 = false;
            true
        } else {
            false
        }
    }

    /// Acorda o consumidor (ex.: para processar um comando).
    pub fn poke(&self) {
        self.cv.notify_one();
    }
}

impl Default for TripleBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DriverCmd {
    Brightness(u32),
    /// 0..=3
    Rotation(u32),
    Shutdown,
}

/// Tempo de CPU (kernel + usuário) consumido pela thread atual.
pub fn thread_cpu_time() -> Duration {
    let (k, u) = thread_cpu_split();
    k + u
}

/// Tempo de CPU da thread atual separado em (kernel, usuário).
pub fn thread_cpu_split() -> (Duration, Duration) {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        if GetThreadTimes(GetCurrentThread(), &mut c, &mut e, &mut k, &mut u).is_err() {
            return (Duration::ZERO, Duration::ZERO);
        }
    }
    let t = |f: FILETIME| Duration::from_nanos((((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64) * 100);
    (t(k), t(u))
}
