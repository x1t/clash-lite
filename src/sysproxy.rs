use crate::ui::wide;
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Networking::WinInet::InternetSetOptionW,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_SZ, RegCloseKey, RegOpenKeyExW,
        RegSetValueExW,
    },
};

const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
const BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";
const OPTION_SETTINGS_CHANGED: u32 = 39;
const OPTION_REFRESH: u32 = 37;

fn open() -> Option<HKEY> {
    let mut key: HKEY = null_mut();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(KEY).as_ptr(),
            0,
            KEY_WRITE,
            &mut key,
        )
    };
    (rc == 0).then_some(key)
}

fn set_dword(key: HKEY, name: &str, value: u32) {
    let bytes = value.to_le_bytes();
    unsafe { RegSetValueExW(key, wide(name).as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4) };
}

fn set_sz(key: HKEY, name: &str, value: &str) {
    let data = wide(value);
    let len = (data.len() * 2) as u32;
    unsafe {
        RegSetValueExW(
            key,
            wide(name).as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            len,
        )
    };
}

fn notify() {
    unsafe {
        InternetSetOptionW(null_mut(), OPTION_SETTINGS_CHANGED, null(), 0);
        InternetSetOptionW(null_mut(), OPTION_REFRESH, null(), 0);
    }
}

pub fn enable(port: u16) {
    let Some(key) = open() else { return };
    set_sz(key, "ProxyServer", &format!("127.0.0.1:{port}"));
    set_sz(key, "ProxyOverride", BYPASS);
    set_dword(key, "ProxyEnable", 1);
    unsafe { RegCloseKey(key) };
    notify();
}

pub fn disable() {
    let Some(key) = open() else { return };
    set_dword(key, "ProxyEnable", 0);
    unsafe { RegCloseKey(key) };
    notify();
}
