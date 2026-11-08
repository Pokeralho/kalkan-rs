//! Player de GIF / WebP / APNG animados.
//!
//! Os frames são decodificados numa thread dedicada, já compostos, recortados
//! para 320x240 e armazenados em RGB565 (150 KB/frame). Se a animação couber no
//! orçamento de cache, após o primeiro ciclo ela é reproduzida inteiramente da
//! RAM (CPU ~0). Caso contrário, a thread decodifica em streaming contínuo.

use super::{fit_image, Fit};
use crate::renderer::canvas::u32_to_rgb565;
use anyhow::Result;
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use image::codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder};
use image::{AnimationDecoder, Frames};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type Pixels565 = Arc<[u16]>;

#[derive(Clone)]
pub struct AnimFrame {
    pub px: Pixels565,
    pub delay: Duration,
}

pub fn is_apng(path: &Path) -> bool {
    File::open(path)
        .ok()
        .and_then(|f| PngDecoder::new(BufReader::new(f)).ok())
        .and_then(|d| d.is_apng().ok())
        .unwrap_or(false)
}

fn open_frames(path: &Path) -> Result<Option<Frames<'static>>> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let r = BufReader::new(File::open(path)?);
    Ok(match ext.as_str() {
        "gif" => Some(GifDecoder::new(r)?.into_frames()),
        "webp" => {
            let d = WebPDecoder::new(r)?;
            if d.has_animation() { Some(d.into_frames()) } else { None }
        }
        _ => {
            let d = PngDecoder::new(r)?;
            if d.is_apng()? { Some(d.apng()?.into_frames()) } else { None }
        }
    })
}

fn delay_of(frame: &image::Frame) -> Duration {
    let (n, d) = frame.delay().numer_denom_ms();
    let ms = if d == 0 { 100 } else { n / d };
    // Mesmo comportamento dos navegadores: atrasos < 20 ms viram 100 ms.
    Duration::from_millis(if ms < 20 { 100 } else { ms as u64 })
}

fn to_565(px: Vec<u32>) -> Pixels565 {
    px.into_iter().map(u32_to_rgb565).collect::<Vec<u16>>().into()
}

fn decode_loop(path: PathBuf, fit: Fit, tx: Sender<AnimFrame>, streaming: Arc<AtomicBool>) {
    loop {
        match open_frames(&path) {
            Ok(Some(frames)) => {
                for f in frames {
                    let f = match f {
                        Ok(f) => f,
                        Err(e) => {
                            log::warn!("{}: frame inválido: {e}", path.display());
                            break;
                        }
                    };
                    let delay = delay_of(&f);
                    let px = to_565(fit_image(&f.into_buffer(), fit));
                    if tx.send(AnimFrame { px, delay }).is_err() {
                        return; // player descartado
                    }
                }
            }
            Ok(None) => {
                // Imagem não animada: um único frame "eterno".
                if let Ok(img) = image::open(&path) {
                    let px = to_565(fit_image(&img.to_rgba8(), fit));
                    let _ = tx.send(AnimFrame { px, delay: Duration::from_secs(3600) });
                }
                return;
            }
            Err(e) => {
                log::error!("{}: {e}", path.display());
                return;
            }
        }
        if !streaming.load(Ordering::Relaxed) {
            return; // primeiro ciclo concluído; o player reproduz do cache
        }
    }
}

pub struct AnimPlayer {
    path: PathBuf,
    fit: Fit,
    rx: Receiver<AnimFrame>,
    streaming: Arc<AtomicBool>,
    cache: Vec<AnimFrame>,
    cache_bytes: usize,
    budget: usize,
    cache_complete: bool,
    idx: usize,
    current: Option<Pixels565>,
    next_due: Instant,
}

impl AnimPlayer {
    pub fn open(path: &Path, fit: Fit, budget: usize) -> Result<Self> {
        // Validação síncrona rápida para reportar erros de arquivo cedo.
        File::open(path)?;
        let streaming = Arc::new(AtomicBool::new(false));
        let rx = Self::spawn(path.to_path_buf(), fit, streaming.clone());
        Ok(Self {
            path: path.to_path_buf(),
            fit,
            rx,
            streaming,
            cache: Vec::new(),
            cache_bytes: 0,
            budget,
            cache_complete: false,
            idx: 0,
            current: None,
            next_due: Instant::now(),
        })
    }

    fn spawn(path: PathBuf, fit: Fit, streaming: Arc<AtomicBool>) -> Receiver<AnimFrame> {
        let (tx, rx) = bounded(4);
        std::thread::Builder::new()
            .name("anim-decoder".into())
            .spawn(move || decode_loop(path, fit, tx, streaming))
            .expect("thread de animação");
        rx
    }

    pub fn current(&self) -> Option<&[u16]> {
        self.current.as_deref()
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        Some(self.next_due)
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        if now < self.next_due {
            return false;
        }
        let frame = if self.cache_complete {
            if self.cache.len() <= 1 && self.current.is_some() {
                self.next_due = now + Duration::from_secs(3600);
                return false;
            }
            self.idx = (self.idx + 1) % self.cache.len();
            self.cache[self.idx].clone()
        } else {
            match self.rx.try_recv() {
                Ok(f) => {
                    if !self.streaming.load(Ordering::Relaxed) {
                        self.cache_bytes += f.px.len() * 2;
                        if self.cache_bytes > self.budget {
                            log::info!(
                                "{}: animação excede o cache ({} MB); modo streaming",
                                self.path.display(),
                                self.budget / 1048576
                            );
                            self.streaming.store(true, Ordering::Relaxed);
                            self.cache = Vec::new();
                        } else {
                            self.cache.push(f.clone());
                        }
                    }
                    f
                }
                Err(TryRecvError::Empty) => {
                    // Decoder ainda não entregou: tenta de novo em breve.
                    self.next_due = now + Duration::from_millis(5);
                    return false;
                }
                Err(TryRecvError::Disconnected) => {
                    if !self.streaming.load(Ordering::Relaxed) && !self.cache.is_empty() {
                        self.cache_complete = true;
                        self.idx = 0;
                        self.cache[0].clone()
                    } else {
                        self.rx = Self::spawn(self.path.clone(), self.fit, self.streaming.clone());
                        self.next_due = now + Duration::from_millis(5);
                        return false;
                    }
                }
            }
        };

        self.current = Some(frame.px);
        // Pacing preciso: agenda a partir do prazo anterior, sem acumular atraso.
        self.next_due += frame.delay;
        if self.next_due + Duration::from_millis(250) < now {
            self.next_due = now + frame.delay;
        }
        true
    }
}
