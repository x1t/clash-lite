//! Downloads mihomo.exe and the dashboard next to our exe, so a bare clash-lite.exe is enough.
use crate::state::{Res, base_dir};
use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use ureq::{Agent, Proxy};

const MIHOMO_VERSION_URL: &str =
    "https://github.com/MetaCubeX/mihomo/releases/latest/download/version.txt";
const UI_ZIP_URL: &str = "https://github.com/x1t/clash-lite/raw/main/assets/yacd.zip";
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn mihomo_path() -> PathBuf {
    base_dir().join("mihomo.exe")
}

pub fn ui_dir() -> PathBuf {
    base_dir().join("ui")
}

/// Same build Clash Verge ships: amd64-v2 runs on any x86-64 CPU from the last ~15 years.
fn mihomo_zip_url(version: &str) -> String {
    format!(
        "https://github.com/MetaCubeX/mihomo/releases/download/{version}/mihomo-windows-amd64-v2-{version}.zip"
    )
}

fn get(url: &str, proxy_port: Option<u16>) -> Res<Vec<u8>> {
    let proxy = match proxy_port {
        Some(port) => Some(Proxy::new(&format!("http://127.0.0.1:{port}"))?),
        None => None,
    };
    let agent: Agent = Agent::config_builder()
        .proxy(proxy)
        .user_agent("clash-lite") // the GitHub API rejects requests without a User-Agent
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    let mut resp = agent.get(url).call()?;
    Ok(resp
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()?)
}

/// Direct first; if GitHub is blocked, retry through our own running core.
pub fn download(url: &str, proxy_port: Option<u16>) -> Res<Vec<u8>> {
    get(url, None).or_else(|direct_err| match proxy_port {
        Some(port) => get(url, Some(port)),
        None => Err(direct_err),
    })
}

/// Unpacks with Windows' bundled bsdtar (System32), not a Git/MSYS tar that cannot read zip.
pub fn extract(zip: &[u8], dest: &Path) -> Res<()> {
    let _ = fs::remove_dir_all(dest);
    fs::create_dir_all(dest)?;
    let archive = dest.with_extension("zip");
    fs::write(&archive, zip)?;
    let tar = Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
        .join(r"System32\tar.exe");
    let out = Command::new(tar)
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(dest)
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let _ = fs::remove_file(&archive);
    let out = out?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("解压失败：{}", String::from_utf8_lossy(&out.stderr).trim()).into())
    }
}

/// e.g. `Mihomo Meta v1.19.32 windows amd64 with go1.26.8 ...` -> `v1.19.32`.
pub fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|t| t.starts_with('v') && t[1..].starts_with(|c: char| c.is_ascii_digit()))
        .map(String::from)
}

pub fn installed_mihomo_version() -> Option<String> {
    let out = Command::new(mihomo_path())
        .arg("-v")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

pub fn latest_mihomo_version(proxy_port: Option<u16>) -> Res<String> {
    let text = String::from_utf8(download(MIHOMO_VERSION_URL, proxy_port)?)?;
    parse_version(&text).ok_or_else(|| format!("无法识别 mihomo 版本号：{}", text.trim()).into())
}

/// Downloads and unpacks `version`; returns the staged exe, leaving the live one untouched.
pub fn stage_mihomo(version: &str, proxy_port: Option<u16>) -> Res<PathBuf> {
    let zip = download(&mihomo_zip_url(version), proxy_port)
        .map_err(|e| format!("下载 mihomo {version} 失败：{e}"))?;
    let dir = base_dir().join("mihomo-staging");
    extract(&zip, &dir)?;
    let exe = fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .ok_or("压缩包里没有 mihomo 可执行文件")?;
    Ok(exe)
}

/// Moves a staged exe into place; the core must be stopped first.
pub fn commit_mihomo(staged: &Path) -> Res<()> {
    let _ = fs::remove_file(mihomo_path());
    fs::rename(staged, mihomo_path())?;
    let _ = fs::remove_dir_all(base_dir().join("mihomo-staging"));
    Ok(())
}

fn install_ui() -> Res<()> {
    let zip = download(UI_ZIP_URL, None).map_err(|e| format!("下载面板失败：{e}"))?;
    let staging = base_dir().join("ui-staging");
    extract(&zip, &staging)?;
    let _ = fs::remove_dir_all(ui_dir());
    fs::rename(staging, ui_dir())?;
    Ok(())
}

/// First run: fetch whatever is missing. The dashboard is optional, so it never blocks the core.
pub fn ensure() -> Res<()> {
    let ui_result = if ui_dir().join("index.html").is_file() {
        Ok(())
    } else {
        install_ui()
    };
    if !mihomo_path().is_file() {
        let version = latest_mihomo_version(None)?;
        commit_mihomo(&stage_mihomo(&version, None)?)?;
    }
    ui_result
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn parses_mihomo_versions() {
        let out = "Mihomo Meta v1.19.32 windows amd64 with go1.26.8 Wed Sep 30 17:00:13 UTC 2026";
        assert_eq!(parse_version(out).as_deref(), Some("v1.19.32"));
        assert_eq!(parse_version("v1.19.33\n").as_deref(), Some("v1.19.33"));
        assert_eq!(parse_version("<html>not found</html>"), None);
    }
}
