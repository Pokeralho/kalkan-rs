//! Decodificador de vídeo (MP4/H.264, HEVC, WMV…) via Windows Media Foundation.
//!
//! Nenhum ffmpeg empacotado: o Source Reader do próprio Windows decodifica
//! (com aceleração quando disponível) e já entrega RGB32 escalonado para o
//! tamanho "cover" da tela; aqui só recortamos o centro em 320x240.
//! Uma thread dedicada mantém uma fila de pré-buffer e faz loop contínuo
//! reposicionando o reader no início ao chegar ao fim do stream.

use super::cover_size;
use crate::driver::{HEIGHT, PIXELS, WIDTH};
use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING, PROPVARIANT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

/// Frames pré-decodificados na fila (~300 KB cada).
const PREBUFFER: usize = 4;

struct VFrame {
    px: Vec<u32>,
    dur: Duration,
}

pub struct VideoPlayer {
    rx: Receiver<VFrame>,
    recycle: Sender<Vec<u32>>,
    current: Option<Vec<u32>>,
    next_due: Instant,
}

impl VideoPlayer {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_file() {
            anyhow::bail!("vídeo não encontrado: {}", path.display());
        }
        let (tx, rx) = bounded(PREBUFFER);
        let (recycle, recycle_rx) = bounded(PREBUFFER + 2);
        let p = path.to_path_buf();
        std::thread::Builder::new()
            .name("video-decoder".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
                        log::error!("MFStartup: {e}");
                        return;
                    }
                }
                if let Err(e) = decode(&p, &tx, &recycle_rx) {
                    log::error!("vídeo {}: {e:#}", p.display());
                }
                unsafe {
                    let _ = MFShutdown();
                    CoUninitialize();
                }
            })?;
        Ok(Self { rx, recycle, current: None, next_due: Instant::now() })
    }

    pub fn current(&self) -> Option<&[u32]> {
        self.current.as_deref()
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        Some(self.next_due)
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        if now < self.next_due {
            return false;
        }
        match self.rx.try_recv() {
            Ok(f) => {
                if let Some(old) = self.current.replace(f.px) {
                    let _ = self.recycle.try_send(old);
                }
                self.next_due += f.dur;
                if self.next_due + Duration::from_millis(250) < now {
                    self.next_due = now + f.dur;
                }
                true
            }
            Err(TryRecvError::Empty) => {
                self.next_due = now + Duration::from_millis(4);
                false
            }
            Err(TryRecvError::Disconnected) => {
                // Decoder terminou com erro: congela no último frame.
                self.next_due = now + Duration::from_secs(3600);
                false
            }
        }
    }
}

struct Layout {
    w: u32,
    h: u32,
    stride: i32,
}

unsafe fn current_layout(reader: &IMFSourceReader, stream: u32) -> Result<Layout> {
    let cur = reader.GetCurrentMediaType(stream)?;
    let fs = cur.GetUINT64(&MF_MT_FRAME_SIZE)?;
    let (w, h) = ((fs >> 32) as u32, fs as u32);
    let stride = cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).unwrap_or((w * 4) as i32);
    Ok(Layout { w, h, stride })
}

fn decode(path: &PathBuf, tx: &Sender<VFrame>, recycle: &Receiver<Vec<u32>>) -> Result<()> {
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 2)?;
        let attrs = attrs.context("MFCreateAttributes")?;
        // Habilita conversão de cor + escala dentro do pipeline do MF.
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
        attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;

        let url = HSTRING::from(path.canonicalize()?.to_string_lossy().trim_start_matches(r"\\?\"));
        let reader = MFCreateSourceReaderFromURL(&url, &attrs).context("abrindo vídeo")?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(stream, true)?;

        let native = reader.GetNativeMediaType(stream, 0)?;
        let fs = native.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (nw, nh) = ((fs >> 32) as u32, fs as u32);
        let fps_default = native
            .GetUINT64(&MF_MT_FRAME_RATE)
            .ok()
            .map(|r| ((r >> 32) as f64, (r & 0xFFFF_FFFF) as f64))
            .filter(|(n, d)| *n > 0.0 && *d > 0.0)
            .map(|(n, d)| Duration::from_secs_f64(d / n))
            .unwrap_or(Duration::from_millis(33));

        // Escala "cover" feita pelo MF; dimensões pares exigidas pelo processador.
        let (mut tw, mut th) = cover_size(nw, nh);
        tw += tw & 1;
        th += th & 1;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, ((tw as u64) << 32) | th as u64)?;
        reader.SetCurrentMediaType(stream, None, &mt).context("formato RGB32 não suportado")?;
        let mut layout = current_layout(&reader, stream)?;
        log::info!(
            "vídeo {}: {nw}x{nh} → {}x{} ({:.1} fps)",
            path.display(),
            layout.w,
            layout.h,
            1.0 / fps_default.as_secs_f64()
        );

        let mut frames_this_pass = 0u32;
        loop {
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(stream, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample))?;

            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                layout = current_layout(&reader, stream)?;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                if frames_this_pass == 0 {
                    anyhow::bail!("vídeo sem frames decodificáveis");
                }
                frames_this_pass = 0;
                // Loop contínuo: volta ao início; a fila de pré-buffer esconde o seek.
                reader.SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(0i64))?;
                continue;
            }
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                anyhow::bail!("erro no stream de vídeo");
            }
            let Some(sample) = sample else { continue };
            frames_this_pass += 1;

            let dur = sample
                .GetSampleDuration()
                .ok()
                .filter(|d| *d > 0)
                .map(|d| Duration::from_nanos(d as u64 * 100))
                .unwrap_or(fps_default);

            let mut px = recycle.try_recv().unwrap_or_else(|_| vec![0u32; PIXELS]);
            let buf = sample.ConvertToContiguousBuffer()?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            buf.Lock(&mut ptr, None, Some(&mut len))?;
            if frames_this_pass == 1 {
                log::debug!("vídeo: buffer {len} B, layout {}x{} stride {}", layout.w, layout.h, layout.stride);
            }
            crop_into(&mut px, ptr, len as usize, &layout);
            buf.Unlock()?;

            if tx.send(VFrame { px, dur }).is_err() {
                return Ok(()); // player descartado (troca de tema)
            }
        }
    }
}

/// Copia o centro do frame RGB32 (X8R8G8B8) para 320x240 0x00RRGGBB.
unsafe fn crop_into(dst: &mut [u32], src: *const u8, len: usize, l: &Layout) {
    let abs_stride = l.stride.unsigned_abs() as usize;
    let ox = (l.w as usize).saturating_sub(WIDTH) / 2;
    let oy = (l.h as usize).saturating_sub(HEIGHT) / 2;
    let cw = WIDTH.min(l.w as usize);
    let ch = HEIGHT.min(l.h as usize);
    for y in 0..ch {
        let sy = oy + y;
        // Stride negativo = imagem bottom-up.
        let row = if l.stride < 0 { l.h as usize - 1 - sy } else { sy };
        let off = row * abs_stride + ox * 4;
        if off + cw * 4 > len {
            break;
        }
        let s = std::slice::from_raw_parts(src.add(off) as *const u32, cw);
        let d = &mut dst[y * WIDTH..y * WIDTH + cw];
        for (d, s) in d.iter_mut().zip(s) {
            *d = *s & 0x00FF_FFFF;
        }
    }
}
