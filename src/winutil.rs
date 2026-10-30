//! Funções do Windows usadas pelo app, direto na API do sistema (sem bibliotecas extras).

#![allow(clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetForegroundWindow, SW_SHOWNORMAL};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

// ---------- Uma cópia só ----------

const SHOW_EVENT: &str = "Local\\ErickFuzer.ControleDoAr.Show";

/// Primeira cópia: devolve o evento que outras cópias usam para pedir o popup.
/// Se já houver uma cópia rodando, pede para ela abrir o popup e devolve `None`.
pub fn single_instance() -> Option<isize> {
    let name = wide(SHOW_EVENT);
    // SAFETY: nome terminado em zero; o handle fica aberto durante toda a vida do app.
    unsafe {
        let ev = CreateEventW(null(), 0, 0, name.as_ptr());
        if ev.is_null() {
            return Some(0); // sem evento, segue sem a proteção
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            SetEvent(ev);
            CloseHandle(ev);
            return None;
        }
        Some(ev as isize)
    }
}

/// Espera, numa thread própria, os pedidos de outras cópias.
pub fn on_show_request(event: isize, f: impl Fn() + Send + 'static) {
    if event == 0 {
        return;
    }
    std::thread::spawn(move || loop {
        // SAFETY: handle válido criado em `single_instance` e nunca fechado.
        let r = unsafe { WaitForSingleObject(event as HANDLE, INFINITE) };
        if r != 0 {
            break;
        }
        f();
    });
}

// ---------- Navegador ----------

/// Abre um endereço no navegador padrão.
pub fn open_url(url: &str) -> Result<(), String> {
    let (op, file) = (wide("open"), wide(url));
    // SAFETY: strings terminadas em zero, vivas durante a chamada.
    let r = unsafe { ShellExecuteW(null_mut(), op.as_ptr(), file.as_ptr(), null(), null(), SW_SHOWNORMAL) };
    // ShellExecute devolve um valor > 32 em caso de sucesso.
    if r as isize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecute falhou ({})", r as isize))
    }
}

// ---------- Tela ----------

/// Área útil de um monitor (sem a barra de tarefas), em pixels físicos, e a escala dele.
#[derive(Clone, Copy, Debug)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f64,
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: ponteiro para variável local.
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
}

/// Área útil do monitor sob o ponto, ou do monitor principal.
pub fn work_area(at: Option<(i32, i32)>) -> Option<WorkArea> {
    let (pt, flag) = match at {
        Some((x, y)) => (POINT { x, y }, MONITOR_DEFAULTTONEAREST),
        None => (POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
    };
    // SAFETY: estruturas locais com o tamanho preenchido.
    unsafe {
        let mon = MonitorFromPoint(pt, flag);
        if mon.is_null() {
            return None;
        }
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let scale = if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) == 0 {
            dx as f64 / 96.0
        } else {
            1.0
        };
        let RECT {
            left,
            top,
            right,
            bottom,
        } = mi.rcWork;
        Some(WorkArea {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
            scale,
        })
    }
}

// ---------- Janela ----------

/// Cantos arredondados do Windows 11 numa janela sem moldura.
pub fn round_corners(hwnd: isize) {
    let pref = DWMWCP_ROUND;
    // SAFETY: HWND da janela do app; ponteiro para valor local do tamanho informado.
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const c_void,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}

pub fn bring_to_front(hwnd: isize) {
    // SAFETY: HWND da janela do app.
    unsafe {
        SetForegroundWindow(hwnd as HWND);
    }
}

// ---------- Iniciar com o Windows ----------

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_NAME: &str = "Controle do Ar";

fn run_value() -> Option<String> {
    let (key, name) = (wide(RUN_KEY), wide(RUN_NAME));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: buffer local com o tamanho em bytes informado.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if r != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn this_exe() -> Option<String> {
    std::env::current_exe().ok().map(|p| format!("\"{}\"", p.display()))
}

pub fn autostart_enabled() -> bool {
    run_value().is_some()
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let (key, name) = (wide(RUN_KEY), wide(RUN_NAME));
    // SAFETY: strings terminadas em zero e buffer com o tamanho em bytes informado.
    let r = unsafe {
        if enabled {
            let value = wide(&this_exe().ok_or("caminho do executável indisponível")?);
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr() as *const c_void,
                (value.len() * 2) as u32,
            )
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
            if r == 2 {
                0 // já não existia
            } else {
                r
            }
        }
    };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("registro do Windows: erro {r}"))
    }
}

/// Se a inicialização automática estiver ligada mas apontando para outro executável
/// (versão antiga ou de desenvolvimento), passa a apontar para este.
pub fn fix_autostart_path() {
    if cfg!(debug_assertions) {
        return; // build de desenvolvimento não mexe no registro sozinho
    }
    if let (Some(current), Some(me)) = (run_value(), this_exe()) {
        if !current.eq_ignore_ascii_case(&me) {
            let _ = set_autostart(true);
        }
    }
}
