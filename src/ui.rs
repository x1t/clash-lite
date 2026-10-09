use std::sync::atomic::{AtomicU32, Ordering};
use tray_icon::Icon;
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW, PostThreadMessageW,
        WM_APP, WM_QUIT,
    },
};

static MAIN_TID: AtomicU32 = AtomicU32::new(0);

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn mark_main_thread() {
    MAIN_TID.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

/// Asks the main thread to re-sync icon and tooltip with the current state.
pub fn refresh() {
    unsafe { PostThreadMessageW(MAIN_TID.load(Ordering::Relaxed), WM_APP, 0, 0) };
}

pub fn quit() {
    unsafe { PostThreadMessageW(MAIN_TID.load(Ordering::Relaxed), WM_QUIT, 0, 0) };
}

pub fn message(text: &str) {
    let (text, title) = (wide(text), wide("Clash Lite"));
    let flags = MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST;
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), flags) };
}

pub fn icon(rgb: [u8; 3]) -> Icon {
    const N: i32 = 32;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x - N / 2, y - N / 2);
            let inside = dx * dx + dy * dy <= (N / 2 - 2) * (N / 2 - 2);
            let alpha = if inside { 255 } else { 0 };
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    Icon::from_rgba(px, N as u32, N as u32).expect("valid icon buffer")
}
