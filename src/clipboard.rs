use windows_sys::Win32::System::{
    DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
    Memory::{GlobalLock, GlobalUnlock},
};

const CF_UNICODETEXT: u32 = 13;

pub fn text() -> Option<String> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let mut out = None;
        let handle = GetClipboardData(CF_UNICODETEXT);
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u16;
            if !ptr.is_null() {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        out
    }
}
