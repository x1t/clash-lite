use crate::{
    api::Api,
    clipboard,
    core::Core,
    state::{Profile, Res, State, now_secs, profile_path, profiles_dir},
    subscription::{self, Fetched},
    sysproxy,
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{Mutex, MutexGuard, TryLockError, mpsc::Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const READY_TIMEOUT: Duration = Duration::from_secs(20);
const FALLBACK_PORT: u16 = 7897;
const RETRY_AFTER: Duration = Duration::from_secs(600);
const MAX_SLEEP: Duration = Duration::from_secs(6 * 3600);

pub struct Inner {
    pub state: State,
    pub core: Core,
    pub port: Option<u16>,
}

pub struct App {
    inner: Mutex<Inner>,
    wake: Sender<()>,
}

fn tun_body(on: bool) -> Value {
    if !on {
        return json!({ "tun": { "enable": false } });
    }
    json!({ "tun": {
        "enable": true, "stack": "mixed", "auto-route": true,
        "auto-detect-interface": true, "dns-hijack": ["any:53"],
    } })
}

impl Inner {
    fn api(&self) -> Api {
        Api::new(&self.state.secret)
    }

    fn wait_ready(&mut self, api: &Api) -> Res<()> {
        let started = std::time::Instant::now();
        while started.elapsed() < READY_TIMEOUT {
            if api.version().is_ok() {
                return Ok(());
            }
            if !self.core.alive() {
                return Err("mihomo 启动后退出了，请查看 mihomo.log".into());
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err("等待 mihomo 就绪超时，请查看 mihomo.log".into())
    }

    /// Picks the proxy port, enforces our TUN choice and re-points the system proxy.
    fn apply_overrides(&mut self, api: &Api) -> Res<()> {
        let cfg = api.configs()?;
        let existing = ["mixed-port", "port"]
            .iter()
            .find_map(|k| cfg[*k].as_u64().filter(|p| *p > 0));
        let port = match existing {
            Some(p) => p as u16,
            None => {
                api.patch_configs(&json!({ "mixed-port": FALLBACK_PORT }))?;
                FALLBACK_PORT
            }
        };
        self.port = Some(port);
        api.patch_configs(&json!({ "log-level": "warning" }))?;
        api.patch_configs(&tun_body(self.state.tun))?;
        if self.state.sysproxy {
            sysproxy::enable(port);
        }
        Ok(())
    }

    /// (Re)starts mihomo with the active profile; stays stopped when there is none.
    fn restart(&mut self) -> Res<()> {
        self.port = None;
        let Some(path) = self.state.active_path() else {
            self.core.stop();
            return Ok(());
        };
        self.core.start(&path, &self.state.secret)?;
        let api = self.api();
        self.wait_ready(&api)?;
        self.apply_overrides(&api)
    }

    /// Activates `id`; on failure falls back to the previous profile.
    fn switch(&mut self, id: &str) -> Res<()> {
        let previous = self.state.active.replace(id.to_string());
        if let Err(e) = self.restart() {
            self.state.active = previous;
            let _ = self.restart();
            return Err(e);
        }
        self.state.save()
    }
}

impl App {
    pub fn new(wake: Sender<()>) -> Self {
        let inner = Inner {
            state: State::load(),
            core: Core::default(),
            port: None,
        };
        Self {
            inner: Mutex::new(inner),
            wake,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Non-blocking access for the UI thread; `None` while a long operation runs.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, Inner>> {
        match self.inner.try_lock() {
            Ok(g) => Some(g),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    pub fn api(&self) -> Api {
        Api::new(&self.lock().state.secret)
    }

    pub fn start(&self) -> Res<()> {
        self.lock().restart()
    }

    pub fn shutdown(&self) {
        let mut g = self.lock();
        // Keep `state.sysproxy` so the next launch restores it; just don't leave a dead proxy behind.
        if g.state.sysproxy {
            sysproxy::disable();
        }
        g.core.stop();
    }

    pub fn switch(&self, id: &str) -> Res<()> {
        self.lock().switch(id)
    }

    pub fn set_tun(&self, on: bool) -> Res<()> {
        let mut g = self.lock();
        if !g.core.alive() {
            return Err("mihomo 未运行，请先添加订阅".into());
        }
        let api = g.api();
        api.patch_configs(&tun_body(on))?;
        thread::sleep(Duration::from_millis(500));
        if api.configs()?["tun"]["enable"].as_bool() != Some(on) {
            return Err("TUN 切换失败：请确认以管理员运行，详见 mihomo.log".into());
        }
        g.state.tun = on;
        g.state.save()
    }

    pub fn toggle_sysproxy(&self) -> Res<()> {
        let mut g = self.lock();
        let on = !g.state.sysproxy;
        if on {
            sysproxy::enable(g.port.ok_or("mihomo 未运行，无法设置系统代理")?);
        } else {
            sysproxy::disable();
        }
        g.state.sysproxy = on;
        g.state.save()
    }

    pub fn toggle_silent(&self) -> Res<()> {
        let mut g = self.lock();
        g.state.show_dashboard = !g.state.show_dashboard;
        g.state.save()
    }

    /// Direct first; if blocked, retry through our own running core.
    fn fetch(&self, url: &str) -> Res<Fetched> {
        subscription::fetch(url, None).or_else(|direct_err| match self.lock().port {
            Some(port) => subscription::fetch(url, Some(port)),
            None => Err(direct_err),
        })
    }

    pub fn add_from_clipboard(&self) -> Res<String> {
        let url = clipboard::text()
            .map(|t| t.trim().to_string())
            .filter(|t| t.starts_with("http://") || t.starts_with("https://"))
            .ok_or("剪贴板里没有订阅链接（需要 http/https 开头）")?;
        let known = self
            .lock()
            .state
            .profiles
            .iter()
            .find(|p| p.url == url)
            .map(|p| p.id.clone());
        if let Some(id) = known {
            self.update(&id)?;
            return Ok("该订阅已存在，已更新".into());
        }
        let fetched = self.fetch(&url)?;
        let id = format!(
            "R{:x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
        );
        let mut profile = Profile {
            id: id.clone(),
            name: fetched.name.clone(),
            url,
            ..Default::default()
        };
        profile.apply(&fetched, now_secs());
        fs::create_dir_all(profiles_dir())?;
        fs::write(profile_path(&id), &fetched.body)?;
        let mut g = self.lock();
        g.state.profiles.push(profile);
        if let Err(e) = g.switch(&id) {
            g.state.profiles.retain(|p| p.id != id);
            let _ = fs::remove_file(profile_path(&id));
            return Err(e);
        }
        drop(g);
        let _ = self.wake.send(());
        Ok(format!("已添加并启用：{}", fetched.name))
    }

    pub fn update(&self, id: &str) -> Res<()> {
        let url = self
            .lock()
            .state
            .profile(id)
            .ok_or("订阅不存在")?
            .url
            .clone();
        let fetched = self.fetch(&url)?;
        let mut g = self.lock();
        let (path, backup) = (
            profile_path(id),
            profile_path(id).with_extension("yaml.bak"),
        );
        let _ = fs::copy(&path, &backup);
        fs::write(&path, &fetched.body)?;
        if let Some(p) = g.state.profiles.iter_mut().find(|p| p.id == id) {
            p.apply(&fetched, now_secs());
        }
        if g.state.active.as_deref() == Some(id)
            && let Err(e) = g.restart()
        {
            let _ = fs::rename(&backup, &path);
            let _ = g.restart();
            return Err(e);
        }
        g.state.save()?;
        drop(g);
        let _ = self.wake.send(());
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Res<()> {
        let mut g = self.lock();
        g.state.profiles.retain(|p| p.id != id);
        let _ = fs::remove_file(profile_path(id));
        if g.state.active.as_deref() == Some(id) {
            g.state.active = g.state.profiles.first().map(|p| p.id.clone());
            g.restart()?;
        }
        g.state.save()
    }

    /// Time until the next profile is due for auto-update.
    pub fn next_due(&self) -> Duration {
        let now = now_secs();
        let remaining = self
            .lock()
            .state
            .profiles
            .iter()
            .filter(|p| p.interval_h > 0)
            .map(|p| (p.updated + p.interval_h * 3600).saturating_sub(now))
            .min();
        match remaining {
            Some(0) => RETRY_AFTER,
            Some(s) => Duration::from_secs(s).min(MAX_SLEEP),
            None => MAX_SLEEP,
        }
    }

    pub fn update_due(&self) {
        let now = now_secs();
        let due: Vec<String> = self
            .lock()
            .state
            .profiles
            .iter()
            .filter(|p| p.interval_h > 0 && p.updated + p.interval_h * 3600 <= now)
            .map(|p| p.id.clone())
            .collect();
        for id in due {
            let _ = self.update(&id);
        }
    }
}
