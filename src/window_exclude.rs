//! Módulo para gerenciamento de transparência de clique e exclusão de janela no Windows.

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND as WIN_HWND;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::ScreenToClient;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, POINT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
    SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};

#[cfg(target_os = "windows")]
const WDA_EXCLUDE_FROM_CAPTURE: WINDOW_DISPLAY_AFFINITY = WINDOW_DISPLAY_AFFINITY(0x00000011);

#[cfg(target_os = "windows")]
pub fn exclude_window_from_capture(hwnd_ptr: isize) -> bool {
    let hwnd = WIN_HWND(hwnd_ptr as _);
    unsafe {
        let result = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDE_FROM_CAPTURE);
        result.is_ok()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn exclude_window_from_capture(_hwnd_ptr: isize) -> bool {
    false
}

#[cfg(target_os = "windows")]
pub fn get_client_cursor_pos(hwnd_ptr: isize) -> Option<(f32, f32)> {
    let hwnd = WIN_HWND(hwnd_ptr as _);
    let mut pt = windows::Win32::Foundation::POINT { x: 0, y: 0 };
    unsafe {
        let sys_pt = &mut pt as *mut _ as *mut POINT;
        if GetCursorPos(sys_pt) == 0 {
            return None;
        }
        if !ScreenToClient(hwnd, &mut pt).as_bool() {
            return None;
        }
    }
    Some((pt.x as f32, pt.y as f32))
}

#[cfg(not(target_os = "windows"))]
pub fn get_client_cursor_pos(_hwnd_ptr: isize) -> Option<(f32, f32)> {
    None
}

/// Ativa ou desativa a passagem de cliques (click-through)
#[cfg(target_os = "windows")]
pub fn set_click_through(hwnd_ptr: isize, enable: bool) {
    let hwnd = hwnd_ptr as HWND;
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = if enable {
            ex_style | (WS_EX_LAYERED | WS_EX_TRANSPARENT) as isize
        } else {
            (ex_style | WS_EX_LAYERED as isize) & !(WS_EX_TRANSPARENT as isize)
        };

        if new_style != ex_style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set_click_through(_hwnd_ptr: isize, _enable: bool) {}
