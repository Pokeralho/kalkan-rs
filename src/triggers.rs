//! Gatilhos: trocam o tema automaticamente.
//!
//! * `process`: um programa está aberto (ou em primeiro plano), ex. `GTA5.exe`.
//! * `condition`: uma expressão fica verdadeira, ex. `gpu_temp >= 80`.
//! * `hotkey`: um atalho global de teclado, ex. `F23` ou `Ctrl+Alt+1`.
//!
//! Gatilhos de processo/condição sobrepõem o tema enquanto estão ativos (o de
//! maior `priority` vence) e o display volta sozinho quando terminam. Atalhos
//! trocam o tema ativo, como escolher na bandeja.

use crate::renderer::expr::Expr;
use crate::telemetry::Snapshot;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn is_false(v: &bool) -> bool {
    !*v
}
fn is_zero_i(v: &i32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TriggerDef {
    /// Nome livre, só para identificação.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `process`, `condition` ou `hotkey`.
    pub kind: String,
    /// Tema a exibir. Em atalhos, também aceita `@next` e `@previous`.
    pub theme: String,
    /// `process`: nome do executável (ex. `GTA5.exe`); vários separados por vírgula.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub process: String,
    /// `process`: só vale com o programa em primeiro plano.
    #[serde(default, skip_serializing_if = "is_false")]
    pub foreground_only: bool,
    /// `condition`: expressão, ex. `gpu_temp >= 80 || cpu_temp >= 90`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
    /// `hotkey`: ex. `F23`, `Ctrl+Alt+1`, `Win+Shift+K`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// Maior vence quando vários estão ativos ao mesmo tempo.
    #[serde(default, skip_serializing_if = "is_zero_i")]
    pub priority: i32,
    /// Segundos que a regra precisa ficar verdadeira antes de trocar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_s: Option<f32>,
    /// Segundos falsa antes de voltar (evita piscar entre temas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_s: Option<f32>,
    /// Desativado sem apagar.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

// ───────────────────────────── Processos ─────────────────────────────

/// Executáveis em execução (minúsculos) e o do primeiro plano.
pub fn scan_processes() -> (HashSet<String>, String) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let mut names = HashSet::new();
    let mut fg = String::new();
    unsafe {
        let mut fg_pid = 0u32;
        let hwnd = GetForegroundWindow();
        if !hwnd.0.is_null() {
            GetWindowThreadProcessId(hwnd, Some(&mut fg_pid));
        }
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return (names, fg) };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
            if e.th32ProcessID == fg_pid {
                fg = name.clone();
            }
            names.insert(name);
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    (names, fg)
}

fn normalize_exe(s: &str) -> String {
    let s = s.trim().to_lowercase();
    if s.is_empty() || s.ends_with(".exe") {
        s
    } else {
        format!("{s}.exe")
    }
}

// ───────────────────────────── Avaliação ─────────────────────────────

struct Rule {
    def: TriggerDef,
    exes: Vec<String>,
    expr: Option<Expr>,
    true_since: Option<Instant>,
    false_since: Option<Instant>,
    active: bool,
}

/// Avalia gatilhos de processo/condição com histerese (hold/release).
pub struct TriggerEngine {
    rules: Vec<Rule>,
    last_active: Option<String>,
}

impl TriggerEngine {
    pub fn new(defs: &[TriggerDef]) -> Self {
        let rules = defs
            .iter()
            .filter(|d| !d.disabled && (d.kind == "process" || d.kind == "condition") && !d.theme.trim().is_empty())
            .map(|d| Rule {
                exes: d.process.split(',').map(normalize_exe).filter(|s| !s.is_empty()).collect(),
                expr: (d.kind == "condition")
                    .then(|| Expr::parse(&d.condition).map_err(|e| log::warn!("gatilho '{}': {e}", d.condition)).ok())
                    .flatten(),
                def: d.clone(),
                true_since: None,
                false_since: None,
                active: false,
            })
            .collect();
        Self { rules, last_active: None }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Tema que deve sobrepor o atual agora (ou `None`).
    pub fn evaluate(&mut self, snap: &Snapshot) -> Option<String> {
        let now = Instant::now();
        let env = |k: &str| -> Option<f64> {
            use chrono::Timelike;
            let t = chrono::Local::now();
            match k {
                "hour" => Some(t.hour() as f64),
                "minute" => Some(t.minute() as f64),
                _ => snap.metric(k),
            }
        };
        for r in &mut self.rules {
            let raw = match r.def.kind.as_str() {
                "process" => r.exes.iter().any(|x| {
                    if r.def.foreground_only {
                        snap.foreground_app == *x
                    } else {
                        snap.processes.contains(x)
                    }
                }),
                _ => r.expr.as_ref().and_then(|e| e.eval(&env)).is_some_and(|v| v != 0.0),
            };
            let default_hold = if r.def.kind == "condition" { 3.0 } else { 0.0 };
            let hold = Duration::from_secs_f32(r.def.hold_s.unwrap_or(default_hold).max(0.0));
            let release = Duration::from_secs_f32(r.def.release_s.unwrap_or(5.0).max(0.0));
            if raw {
                r.false_since = None;
                let since = *r.true_since.get_or_insert(now);
                if !r.active && now.duration_since(since) >= hold {
                    r.active = true;
                }
            } else {
                r.true_since = None;
                let since = *r.false_since.get_or_insert(now);
                if r.active && now.duration_since(since) >= release {
                    r.active = false;
                }
            }
        }
        let winner = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.active)
            // Maior prioridade; empate: o que aparece antes na lista.
            .max_by_key(|(i, r)| (r.def.priority, std::cmp::Reverse(*i)))
            .map(|(_, r)| (r.def.theme.clone(), r.def.name.clone()));
        let theme = winner.as_ref().map(|w| w.0.clone());
        if theme != self.last_active {
            match &winner {
                Some((t, n)) => log::info!("gatilho ativo{}: tema '{t}'", if n.is_empty() { String::new() } else { format!(" ({n})") }),
                None => log::info!("gatilhos inativos: voltando ao tema normal"),
            }
            self.last_active = theme.clone();
        }
        theme
    }
}

// ───────────────────────────── Atalhos ─────────────────────────────

/// Converte `Ctrl+Alt+F23` em (modificadores, tecla virtual) para `RegisterHotKey`.
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    const MOD_ALT: u32 = 0x1;
    const MOD_CONTROL: u32 = 0x2;
    const MOD_SHIFT: u32 = 0x4;
    const MOD_WIN: u32 = 0x8;
    let mut mods = 0u32;
    let mut vk = None;
    for part in s.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        match part.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "super" | "meta" => mods |= MOD_WIN,
            k => {
                vk = Some(match k {
                    _ if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => k.to_ascii_uppercase().as_bytes()[0] as u32,
                    _ if k.starts_with('f') && k[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)) => 0x6F + k[1..].parse::<u32>().unwrap(),
                    _ if k.starts_with("num") && k[3..].parse::<u32>().is_ok_and(|n| n <= 9) => 0x60 + k[3..].parse::<u32>().unwrap(),
                    "pause" => 0x13,
                    "scrolllock" => 0x91,
                    "insert" => 0x2D,
                    "home" => 0x24,
                    "end" => 0x23,
                    "pageup" => 0x21,
                    "pagedown" => 0x22,
                    "printscreen" | "print" => 0x2C,
                    _ => return None,
                });
            }
        }
    }
    vk.map(|v| (mods, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("F23"), Some((0, 0x86)));
        assert_eq!(parse_hotkey("ctrl+alt+1"), Some((0x3, b'1' as u32)));
        assert_eq!(parse_hotkey("Win+Shift+K"), Some((0xC, b'K' as u32)));
        assert_eq!(parse_hotkey("Num5"), Some((0, 0x65)));
        assert_eq!(parse_hotkey("Ctrl+"), None);
        assert_eq!(parse_hotkey("Ctrl+Banana"), None);
    }

    #[test]
    fn process_and_condition_with_hysteresis() {
        let defs = vec![
            TriggerDef {
                name: "gta".into(),
                kind: "process".into(),
                theme: "cyberpunk".into(),
                process: "GTA5".into(),
                foreground_only: false,
                condition: String::new(),
                key: String::new(),
                priority: 1,
                hold_s: Some(0.0),
                release_s: Some(0.0),
                disabled: false,
            },
            TriggerDef {
                name: "quente".into(),
                kind: "condition".into(),
                theme: "alerta".into(),
                process: String::new(),
                foreground_only: false,
                condition: "gpu_temp >= 80".into(),
                key: String::new(),
                priority: 5,
                hold_s: Some(0.0),
                release_s: Some(0.0),
                disabled: false,
            },
        ];
        let mut eng = TriggerEngine::new(&defs);
        let mut s = Snapshot::default();
        assert_eq!(eng.evaluate(&s), None);
        s.processes = std::sync::Arc::new(["gta5.exe".to_string()].into_iter().collect());
        assert_eq!(eng.evaluate(&s).as_deref(), Some("cyberpunk"));
        s.gpu_temp = Some(85.0);
        assert_eq!(eng.evaluate(&s).as_deref(), Some("alerta"), "maior prioridade vence");
        s.gpu_temp = Some(60.0);
        assert_eq!(eng.evaluate(&s).as_deref(), Some("cyberpunk"));
        s.processes = Default::default();
        assert_eq!(eng.evaluate(&s), None);
    }
}
