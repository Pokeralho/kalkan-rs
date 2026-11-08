//! "Iniciar com o Windows": entrada em `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Vale só para o usuário atual e não precisa de administrador.

use std::path::{Path, PathBuf};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("Kalkan-RS");

/// Comando registrado atualmente (se houver).
pub fn registered_command() -> Option<String> {
    unsafe {
        let mut size = 0u32;
        if RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, RRF_RT_REG_SZ, None, None, Some(&mut size)).is_err() || size == 0 {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 1];
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        Some(String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string())
    }
}

/// Comando que deve estar registrado para o executável atual.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

fn current_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Liga/desliga. Ao ligar, aponta para o executável em uso (corrige caminhos antigos).
pub fn set(enabled: bool) -> windows::core::Result<()> {
    unsafe {
        if enabled {
            let Some(exe) = current_exe() else { return Ok(()) };
            let cmd = HSTRING::from(command_for(&exe));
            let bytes = (cmd.len() + 1) * 2;
            RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, REG_SZ.0, Some(cmd.as_ptr() as *const _), bytes as u32).ok()
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE);
            // Já não existir não é erro.
            if r.is_ok() || r.0 == 2 {
                Ok(())
            } else {
                r.ok()
            }
        }
    }
}

/// Garante que o registro reflita a configuração (e o caminho atual do executável).
pub fn sync(enabled: bool) {
    let want = current_exe().map(|e| command_for(&e));
    let have = registered_command();
    let ok = match (enabled, &have) {
        (true, Some(h)) => Some(h) == want.as_ref(),
        (false, None) => true,
        _ => false,
    };
    if !ok {
        match set(enabled) {
            Ok(()) => log::info!("iniciar com o Windows: {}", if enabled { "ligado" } else { "desligado" }),
            Err(e) => log::warn!("não foi possível alterar a inicialização com o Windows: {e}"),
        }
    }
}
