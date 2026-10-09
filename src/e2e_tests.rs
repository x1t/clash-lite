//! End-to-end: real HTTP subscription server, real clipboard, real mihomo.exe downloaded from GitHub.
use crate::{
    app::App,
    assets, clipboard,
    state::{base_dir, profile_path},
};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command,
    sync::{Arc, Mutex, mpsc},
    thread,
};

fn yaml(version: &str) -> String {
    format!(
        "# {version}\nmixed-port: 17891\nmode: rule\nproxies:\n  - {{name: direct-a, type: direct}}\n  - {{name: direct-b, type: direct}}\nproxy-groups:\n  - {{name: PROXY, type: select, proxies: [direct-a, direct-b, DIRECT]}}\nrules:\n  - MATCH,PROXY\n"
    )
}

/// Serves `body` (swappable) over HTTP with real subscription headers.
fn serve(body: Arc<Mutex<String>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}/sub&token=abc", listener.local_addr().unwrap());
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let body = body.lock().unwrap().clone();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/yaml\r\ncontent-length: {}\r\nconnection: close\r\n\
                 subscription-userinfo: upload=1; download=2; total=3000; expire=1893456000\r\n\
                 profile-update-interval: 12\r\n\
                 content-disposition: attachment; filename*=UTF-8''%E6%B5%8B%E8%AF%95.yaml\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    url
}

fn set_clipboard(text: &str) {
    let script = format!("Set-Clipboard -Value '{text}'");
    let ok = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .unwrap();
    assert!(ok.success(), "Set-Clipboard failed");
}

fn group_now(app: &App) -> String {
    let proxies: Value = app.api().proxies().expect("proxies");
    proxies["proxies"]["PROXY"]["now"]
        .as_str()
        .unwrap()
        .to_string()
}

const OLD_MIHOMO: &str = "v1.19.31";

#[test]
#[ignore = "needs GitHub access and a desktop clipboard; run with --include-ignored"]
fn subscription_lifecycle_with_real_mihomo() {
    for leftover in ["state.json", "mihomo.exe"] {
        let _ = fs::remove_file(base_dir().join(leftover));
    }
    for leftover in ["profiles", "data", "ui"] {
        let _ = fs::remove_dir_all(base_dir().join(leftover));
    }
    // The bundled dashboard zip unpacks with the same code the app uses.
    let zip = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/yacd.zip")).unwrap();
    assets::extract(&zip, &assets::ui_dir()).expect("extract ui");
    assert!(assets::ui_dir().join("index.html").is_file());
    // Start on an older real release so the in-app updater has something to do.
    let old = assets::stage_mihomo(OLD_MIHOMO, None).expect("download old mihomo");
    assets::commit_mihomo(&old).unwrap();
    assert_eq!(
        assets::installed_mihomo_version().as_deref(),
        Some(OLD_MIHOMO)
    );
    let saved_clipboard = clipboard::text();

    let body = Arc::new(Mutex::new(yaml("v1")));
    let url = serve(Arc::clone(&body));
    set_clipboard(&url);
    let (tx, _rx) = mpsc::channel();
    let app = App::new(tx);

    // add from clipboard: fetch, parse headers, store, start core, select via API
    let msg = app.add_from_clipboard().expect("add");
    assert!(
        msg.contains("测试.yaml"),
        "name from Content-Disposition: {msg}"
    );
    let (id, profile) = {
        let g = app.lock();
        assert_eq!(
            g.state.active.as_deref(),
            Some(g.state.profiles[0].id.as_str())
        );
        (g.state.profiles[0].id.clone(), g.state.profiles[0].clone())
    };
    assert_eq!(
        (profile.interval_h, profile.total, profile.expire),
        (12, 3000, 1_893_456_000)
    );
    assert_eq!(app.lock().port, Some(17891));
    assert_eq!(group_now(&app), "direct-a");
    app.api().select("PROXY", "direct-b").expect("select");
    assert_eq!(group_now(&app), "direct-b");
    app.api().set_mode("global").expect("mode");
    assert_eq!(app.api().configs().unwrap()["mode"], "global");

    // the right-click menu reflects the live core: subscription list and the PROXY group
    let titles: Vec<String> = crate::menu::build(&app)
        .items()
        .iter()
        .filter_map(|i| i.as_submenu().map(|s| s.text()))
        .collect();
    assert!(titles.contains(&"订阅".to_string()), "{titles:?}");
    assert!(
        titles.iter().any(|t| t == "PROXY  [direct-b]"),
        "{titles:?}"
    );
    assert!(titles.contains(&"模式".to_string()), "{titles:?}");

    // bundled Yacd is served by mihomo via -ext-ui
    let mut page = ureq::get(format!("http://127.0.0.1:{}/ui/", crate::core::CTL_PORT))
        .call()
        .expect("ui");
    assert!(page.body_mut().read_to_string().unwrap().contains("yacd"));

    // same URL again updates instead of duplicating
    *body.lock().unwrap() = yaml("v2");
    app.add_from_clipboard().expect("re-add");
    assert_eq!(app.lock().state.profiles.len(), 1);
    assert!(
        fs::read_to_string(profile_path(&id))
            .unwrap()
            .starts_with("# v2")
    );
    assert!(app.lock().core.alive());

    // a broken update must roll back and keep the core running
    *body.lock().unwrap() = "not a clash config".into();
    assert!(app.update(&id).is_err());
    assert!(
        fs::read_to_string(profile_path(&id))
            .unwrap()
            .starts_with("# v2")
    );
    assert!(app.lock().core.alive());

    // looks like a profile but mihomo rejects it: file restored, previous config running again
    *body.lock().unwrap() = "proxies:\n  - {name: x, type: bogus}\n".into();
    assert!(app.update(&id).is_err());
    assert!(
        fs::read_to_string(profile_path(&id))
            .unwrap()
            .starts_with("# v2")
    );
    assert!(app.lock().core.alive());
    assert_eq!(
        group_now(&app),
        "direct-b",
        "mihomo keeps the selection across restarts"
    );

    // in-app updater: download latest while running, swap the exe, restart on the same profile
    let msg = app.update_mihomo().expect("update mihomo");
    let latest = assets::latest_mihomo_version(None).unwrap();
    assert!(msg.contains(&latest), "{msg}");
    assert_eq!(assets::installed_mihomo_version(), Some(latest.clone()));
    assert!(app.lock().core.alive());
    assert_eq!(group_now(&app), "direct-b");
    assert!(app.update_mihomo().unwrap().contains("已是最新"));

    app.delete(&id).expect("delete");
    assert!(app.lock().state.profiles.is_empty());
    assert!(!app.lock().core.alive());
    app.shutdown();
    if let Some(text) = saved_clipboard {
        set_clipboard(&text.replace('\'', "''"));
    }
}
