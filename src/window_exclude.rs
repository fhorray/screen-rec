//! Módulo para ocultar a janela da aplicação da gravação de tela
//! usando a API nativa do Windows `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`.

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
};

/// WDA_EXCLUDEFROMCAPTURE = 0x00000011 (Windows 10 versão 2004 e superior)
#[cfg(target_os = "windows")]
const WDA_EXCLUDE_FROM_CAPTURE: WINDOW_DISPLAY_AFFINITY = WINDOW_DISPLAY_AFFINITY(0x00000011);

#[cfg(target_os = "windows")]
pub fn exclude_window_from_capture(hwnd_ptr: isize) -> bool {
    let hwnd = HWND(hwnd_ptr as _);
    unsafe {
        let result = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDE_FROM_CAPTURE);
        result.is_ok()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn exclude_window_from_capture(_hwnd_ptr: isize) -> bool {
    false
}
