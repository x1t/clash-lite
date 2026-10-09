use crate::state::{Res, base_dir};
use std::{fs, os::windows::process::CommandExt, process::Command};

const AUTOSTART: &str = "ClashLite";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schtasks(args: &[&str]) -> Res<bool> {
    let out = Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(out.status.success())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn exe_path() -> Res<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

/// Logon task with highest privileges (no UAC prompt) that also runs on battery.
fn task_xml(exe: &str, dir: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><WorkingDirectory>{dir}</WorkingDirectory></Exec></Actions>
</Task>"#
    )
}

fn create() -> Res<()> {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let xml = task_xml(
        &xml_escape(&exe_path()?),
        &xml_escape(&base_dir().to_string_lossy()),
        &xml_escape(&user),
    );
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let file = std::env::temp_dir().join("clash-lite-task.xml");
    fs::write(&file, bytes)?;
    let created = schtasks(&[
        "/Create",
        "/TN",
        AUTOSTART,
        "/XML",
        &file.to_string_lossy(),
        "/F",
    ]);
    let _ = fs::remove_file(&file);
    if created? {
        Ok(())
    } else {
        Err("创建计划任务失败，请以管理员身份运行".into())
    }
}

pub fn enabled() -> bool {
    schtasks(&["/Query", "/TN", AUTOSTART]).unwrap_or(false)
}

pub fn set(on: bool) -> Res<()> {
    if on {
        return create();
    }
    if schtasks(&["/Delete", "/TN", AUTOSTART, "/F"])? {
        Ok(())
    } else {
        Err("删除计划任务失败".into())
    }
}
