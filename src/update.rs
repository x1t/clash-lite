//! One-click self-update: replace the running clash-lite.exe with the latest GitHub release.
//!
//! Windows locks a running executable, so (like mihomo's core updater) we rename the live
//! exe aside, drop the new one in its place, launch it, and quit. The fresh process waits
//! for us to exit, deletes the leftover, then starts normally.
use crate::{assets, state::Res, ui};
use serde_json::Value;
use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

const RELEASE_API: &str = "https://api.github.com/repos/x1t/clash-lite/releases/latest";
const ASSET_NAME: &str = "clash-lite.exe";
const POST_UPDATE_FLAG: &str = "--post-update";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MIN_EXE_BYTES: usize = 256 * 1024;

fn old_path() -> Res<PathBuf> {
    Ok(std::env::current_exe()?.with_extension("exe.old"))
}

fn new_path() -> Res<PathBuf> {
    Ok(std::env::current_exe()?.with_extension("exe.new"))
}

/// Parses `v0.1.2` / `0.1.2` into comparable numbers; trailing junk is ignored.
fn parse_version(tag: &str) -> Option<(u32, u32, u32)> {
    let mut parts = tag.trim().trim_start_matches('v').split('.');
    let mut next = || {
        parts
            .next()?
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    };
    Some((next()?, next()?, next()?))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

struct Release {
    tag: String,
    asset_url: String,
}

fn latest_release(proxy_port: Option<u16>) -> Res<Release> {
    let body = assets::download(RELEASE_API, proxy_port)?;
    let json: Value = serde_json::from_slice(&body)?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or("GitHub 返回里没有版本号")?
        .to_string();
    let asset_url = json["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str() == Some(ASSET_NAME))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or("最新发行版里没有 clash-lite.exe")?
        .to_string();
    Ok(Release { tag, asset_url })
}

/// Rejects an HTML error page or a truncated download before it can overwrite the real exe.
fn is_valid_exe(bytes: &[u8]) -> Res<()> {
    if bytes.len() >= MIN_EXE_BYTES && bytes.starts_with(b"MZ") {
        Ok(())
    } else {
        Err("下载的文件不是有效的程序，已取消更新".into())
    }
}

/// Writes `bytes` to `new`, renames the live `exe` aside to `old`, then moves `new` into place.
/// On the final failure it restores `exe` from `old` so the app stays runnable.
fn swap_in_place(exe: &Path, old: &Path, new: &Path, bytes: &[u8]) -> Res<()> {
    let _ = fs::remove_file(old);
    fs::write(new, bytes)?;
    fs::rename(exe, old)?;
    if let Err(e) = fs::rename(new, exe) {
        let _ = fs::rename(old, exe);
        let _ = fs::remove_file(new);
        return Err(format!("替换程序失败：{e}").into());
    }
    Ok(())
}

/// Swaps the new exe into place, relaunches it, and signals this process to quit.
/// Returns `Ok(Some(msg))` only when already up to date — otherwise it never returns normally.
pub fn run(proxy_port: Option<u16>) -> Res<Option<String>> {
    if cfg!(debug_assertions) {
        return Err("调试版不支持自我更新，请用正式版".into());
    }
    let current = env!("CARGO_PKG_VERSION");
    let release = latest_release(proxy_port)?;
    if !is_newer(&release.tag, current) {
        return Ok(Some(format!("已是最新版本 v{current}")));
    }

    let bytes = assets::download(&release.asset_url, proxy_port)
        .map_err(|e| format!("下载 {} 失败：{e}", release.tag))?;
    is_valid_exe(&bytes)?;
    let exe = std::env::current_exe()?;
    swap_in_place(&exe, &old_path()?, &new_path()?, &bytes)?;

    ui::message(&format!("已下载 {}，点击确定后重启生效", release.tag));
    Command::new(&exe)
        .arg(POST_UPDATE_FLAG)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动新版本失败：{e}"))?;
    ui::quit();
    Ok(None)
}

/// New process after an update: wait for the old exe to exit (its `.old` image stays locked
/// until then), delete it, and carry on with a normal launch.
pub fn finish_post_update() {
    let Ok(old) = old_path() else { return };
    for _ in 0..50 {
        if !old.exists() || std::fs::remove_file(&old).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Best-effort cleanup of a leftover `.old` from a previous update, on every normal launch.
pub fn cleanup_leftover() {
    if let Ok(old) = old_path() {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::{is_newer, is_valid_exe, parse_version, swap_in_place};

    #[test]
    fn rejects_non_exe_downloads() {
        assert!(is_valid_exe(b"<html>404</html>").is_err());
        assert!(is_valid_exe(&[0u8; 300 * 1024]).is_err()); // right size, wrong magic
        let mut pe = vec![0u8; 300 * 1024];
        pe[0] = b'M';
        pe[1] = b'Z';
        assert!(is_valid_exe(&pe).is_ok());
    }

    #[test]
    fn swap_places_new_and_keeps_backup() {
        let dir = std::env::temp_dir().join(format!("cl-swap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (exe, old, new) = (
            dir.join("a.exe"),
            dir.join("a.exe.old"),
            dir.join("a.exe.new"),
        );
        std::fs::write(&exe, b"OLD").unwrap();
        swap_in_place(&exe, &old, &new, b"NEW").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"NEW"); // live exe is the new build
        assert_eq!(std::fs::read(&old).unwrap(), b"OLD"); // original kept for the handoff
        assert!(!new.exists()); // staging file consumed
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_and_compares_versions() {
        assert_eq!(parse_version("v0.0.2"), Some((0, 0, 2)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v0.0.2-beta1"), Some((0, 0, 2)));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn detects_newer_only_when_greater() {
        assert!(is_newer("v0.0.3", "0.0.2"));
        assert!(is_newer("v0.1.0", "0.0.9"));
        assert!(!is_newer("v0.0.2", "0.0.2"));
        assert!(!is_newer("v0.0.1", "0.0.2"));
        assert!(!is_newer("garbage", "0.0.2"));
    }
}
