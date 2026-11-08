//! Letras sincronizadas via [LRCLIB](https://lrclib.net) (gratuito, sem chave).
//!
//! Uma thread dedicada busca a letra quando a faixa muda; o resultado fica em
//! cache na memória. Só título, artista, álbum e duração são enviados.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Web::Http::HttpClient;

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    /// (início em segundos, texto). Vazio = trecho instrumental.
    pub lines: Vec<(f64, String)>,
    /// `false` = letra sem tempos (exibida proporcionalmente ao progresso).
    pub synced: bool,
}

impl Lyrics {
    /// Índice da linha atual para a posição dada (segundos).
    pub fn index_at(&self, pos: f64, duration: Option<f64>) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        if self.synced {
            // Pequeno adiantamento compensa a latência do display.
            let p = pos + 0.25;
            match self.lines.partition_point(|(t, _)| *t <= p) {
                0 => None,
                n => Some(n - 1),
            }
        } else {
            let d = duration.filter(|d| *d > 0.0)?;
            Some(((pos / d) * self.lines.len() as f64).floor().clamp(0.0, (self.lines.len() - 1) as f64) as usize)
        }
    }
}

/// Faz o parse de LRC: `[mm:ss.xx] texto`, com múltiplas marcações por linha.
pub fn parse_lrc(src: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            if let Some((m, s)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (m.trim().parse::<f64>(), s.trim().parse::<f64>()) {
                    stamps.push(m * 60.0 + s);
                }
            }
            rest = &stripped[end + 1..];
        }
        let text = rest.trim().to_string();
        for t in stamps {
            out.push((t, text.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

/// Remove sufixos comuns que atrapalham a busca ("- Remastered 2011", "(feat. X)").
fn clean_title(t: &str) -> String {
    let mut s = t.to_string();
    for sep in [" - ", " (", " ["] {
        if let Some(i) = s.find(sep) {
            let tail = s[i..].to_lowercase();
            if ["remaster", "feat", "live", "version", "edit", "mix", "ao vivo", "official"].iter().any(|k| tail.contains(k)) {
                s.truncate(i);
            }
        }
    }
    s.trim().to_string()
}

fn http_get(client: &HttpClient, url: &str) -> windows::core::Result<String> {
    Ok(client.GetStringAsync(&Uri::CreateUri(&HSTRING::from(url))?)?.get()?.to_string_lossy())
}

fn from_json(v: &serde_json::Value) -> Option<Lyrics> {
    if let Some(s) = v["syncedLyrics"].as_str().filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(s);
        if !lines.is_empty() {
            return Some(Lyrics { lines, synced: true });
        }
    }
    let plain = v["plainLyrics"].as_str().filter(|s| !s.trim().is_empty())?;
    Some(Lyrics { lines: plain.lines().map(|l| (0.0, l.trim().to_string())).collect(), synced: false })
}

fn fetch(client: &HttpClient, q: &Query) -> Option<Lyrics> {
    let title = clean_title(&q.title);
    let mut url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        enc(&q.artist),
        enc(&title)
    );
    if !q.album.is_empty() {
        url += &format!("&album_name={}", enc(&q.album));
    }
    if let Some(d) = q.duration.filter(|d| *d > 0.0) {
        url += &format!("&duration={}", d.round() as u64);
    }
    if let Some(l) = http_get(client, &url).ok().and_then(|b| serde_json::from_str(&b).ok()).and_then(|v| from_json(&v)) {
        return Some(l);
    }
    // Busca aproximada: prefere resultados sincronizados com duração próxima.
    let url = format!("https://lrclib.net/api/search?track_name={}&artist_name={}", enc(&title), enc(&q.artist));
    let body = http_get(client, &url).ok()?;
    let arr: Vec<serde_json::Value> = serde_json::from_str(&body).ok()?;
    let score = |v: &serde_json::Value| {
        let synced = v["syncedLyrics"].as_str().is_some_and(|s| !s.is_empty()) as i32 * 1000;
        let dd = match (q.duration, v["duration"].as_f64()) {
            (Some(a), Some(b)) => (a - b).abs().min(999.0) as i32,
            _ => 500,
        };
        synced - dd
    };
    arr.iter().max_by_key(|v| score(v)).and_then(from_json)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<f64>,
}

/// Serviço de letras: `request` dispara a busca em background; `get` devolve
/// o resultado (se pronto) para a chave da faixa.
pub struct LyricsService {
    tx: crossbeam_channel::Sender<Query>,
    cache: Arc<Mutex<HashMap<String, Option<Arc<Lyrics>>>>>,
    last: Option<String>,
}

impl LyricsService {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<Query>();
        let cache: Arc<Mutex<HashMap<String, Option<Arc<Lyrics>>>>> = Arc::default();
        let c = cache.clone();
        std::thread::Builder::new()
            .name("lyrics".into())
            .spawn(move || {
                let client = match HttpClient::new() {
                    Ok(cl) => {
                        if let Ok(h) = cl.DefaultRequestHeaders() {
                            let _ = h.UserAgent().and_then(|ua| ua.TryParseAdd(&HSTRING::from("kalkan-rs (display AIO; letras via LRCLIB)")));
                        }
                        cl
                    }
                    Err(e) => {
                        log::warn!("letras indisponíveis: {e}");
                        return;
                    }
                };
                while let Ok(mut q) = rx.recv() {
                    // Só a faixa mais recente importa.
                    while let Ok(newer) = rx.try_recv() {
                        q = newer;
                    }
                    if c.lock().unwrap().contains_key(&q.key) {
                        continue;
                    }
                    let res = fetch(&client, &q).map(Arc::new);
                    log::info!(
                        "letra '{}' — {}: {}",
                        q.title,
                        q.artist,
                        match &res {
                            Some(l) if l.synced => format!("{} linhas sincronizadas", l.lines.len()),
                            Some(l) => format!("{} linhas (sem tempo)", l.lines.len()),
                            None => "não encontrada".into(),
                        }
                    );
                    let mut g = c.lock().unwrap();
                    if g.len() > 64 {
                        g.clear();
                    }
                    g.insert(q.key.clone(), res);
                }
            })
            .expect("thread de letras");
        Self { tx, cache, last: None }
    }

    pub fn request(&mut self, q: Query) {
        if self.last.as_deref() != Some(&q.key) {
            self.last = Some(q.key.clone());
            let _ = self.tx.send(q);
        }
    }

    pub fn get(&self, key: &str) -> Option<Arc<Lyrics>> {
        self.cache.lock().unwrap().get(key).cloned().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_and_index() {
        let l = parse_lrc("[ar:Banda]\n[00:01.00]Um\n[00:05.50][00:20.00]Dois\n[00:10.00]\n");
        assert_eq!(l.len(), 4);
        assert_eq!(l[1], (5.5, "Dois".into()));
        let ly = Lyrics { lines: l, synced: true };
        assert_eq!(ly.index_at(0.2, None), None);
        assert_eq!(ly.index_at(6.0, None), Some(1));
        assert_eq!(ly.index_at(30.0, None), Some(3));
        assert_eq!(clean_title("Song - Remastered 2011"), "Song");
        assert_eq!(clean_title("Hey (feat. X)"), "Hey");
        assert_eq!(clean_title("Rock - Paper"), "Rock - Paper");
        assert_eq!(enc("a b&c"), "a%20b%26c");
    }
}
