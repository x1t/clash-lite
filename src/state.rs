use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::RandomState,
    fs,
    hash::{BuildHasher, Hasher},
    path::PathBuf,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub url: String,
    pub updated: u64,
    pub interval_h: u64,
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: u64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct State {
    pub secret: String,
    pub active: Option<String>,
    pub tun: bool,
    pub sysproxy: bool,
    /// Off by default: silent start. When on, the dashboard opens at launch.
    pub show_dashboard: bool,
    pub profiles: Vec<Profile>,
}

/// Directory of the executable; everything (core, data, profiles) lives next to it.
pub fn base_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().expect("current_exe");
        exe.parent().expect("exe dir").to_path_buf()
    })
}

pub fn profiles_dir() -> PathBuf {
    base_dir().join("profiles")
}

pub fn profile_path(id: &str) -> PathBuf {
    profiles_dir().join(format!("{id}.yaml"))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn random_hex() -> String {
    let a = RandomState::new().build_hasher().finish();
    let b = RandomState::new().build_hasher().finish();
    format!("{a:016x}{b:016x}")
}

fn file() -> PathBuf {
    base_dir().join("state.json")
}

impl State {
    pub fn load() -> Self {
        let mut state: State = fs::read_to_string(file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if state.secret.is_empty() {
            state.secret = random_hex();
        }
        state
    }

    pub fn save(&self) -> Res<()> {
        fs::create_dir_all(profiles_dir())?;
        let tmp = file().with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, file())?;
        Ok(())
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.active.as_deref().map(profile_path)
    }
}
