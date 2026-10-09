use crate::state::{Res, base_dir};
use std::{
    fs::{self, OpenOptions},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    sync::OnceLock,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

pub const CTL_HOST: &str = "127.0.0.1";
/// Tests use another port so they never collide with a running copy of the app.
pub const CTL_PORT: u16 = if cfg!(test) { 19098 } else { 19097 };
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Default)]
pub struct Core {
    child: Option<Child>,
}

/// Job that kills mihomo when we exit or crash, so no orphan keeps the controller port.
fn job() -> isize {
    static JOB: OnceLock<isize> = OnceLock::new();
    *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as isize
    })
}

fn open_log(path: &Path) -> Res<fs::File> {
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
}

impl Core {
    pub fn start(&mut self, profile: &Path, secret: &str) -> Res<()> {
        self.stop();
        let data = base_dir().join("data");
        fs::create_dir_all(&data)?;
        let log = open_log(&base_dir().join("mihomo.log"))?;
        let mut cmd = Command::new(base_dir().join("mihomo.exe"));
        cmd.arg("-d").arg(&data).arg("-f").arg(profile);
        cmd.arg("-ext-ctl").arg(format!("{CTL_HOST}:{CTL_PORT}"));
        cmd.args(["-secret", secret]);
        let ui = base_dir().join("ui");
        if ui.is_dir() {
            cmd.arg("-ext-ui").arg(ui);
        }
        let child = cmd
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("启动 mihomo.exe 失败: {e}（请把 mihomo.exe 放在程序同目录）"))?;
        unsafe { AssignProcessToJobObject(job() as _, child.as_raw_handle()) };
        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn alive(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.stop();
    }
}
