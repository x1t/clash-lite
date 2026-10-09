use crate::{
    app::App,
    autostart,
    state::{Profile, base_dir},
    ui,
};
use serde_json::Value;
use std::{sync::Arc, thread};
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

/// Separates group and node in a menu id; cannot appear in proxy names.
const SEP: char = '\u{1f}';

struct Snapshot {
    profiles: Vec<Profile>,
    active: Option<String>,
    tun: bool,
    sysproxy: bool,
    show_dashboard: bool,
    running: bool,
}

fn snapshot(app: &App) -> Option<Snapshot> {
    let mut g = app.try_lock()?;
    Some(Snapshot {
        profiles: g.state.profiles.clone(),
        active: g.state.active.clone(),
        tun: g.state.tun,
        sysproxy: g.state.sysproxy,
        show_dashboard: g.state.show_dashboard,
        running: g.core.alive(),
    })
}

fn item(menu: &Menu, id: &str, text: &str, enabled: bool) {
    let _ = menu.append(&MenuItem::with_id(id, text, enabled, None));
}

fn check(menu: &Menu, id: &str, text: &str, on: bool) {
    let _ = menu.append(&CheckMenuItem::with_id(id, text, true, on, None));
}

fn fmt_bytes(n: u64) -> String {
    const G: f64 = 1024.0 * 1024.0 * 1024.0;
    let g = n as f64 / G;
    if g >= 1024.0 {
        format!("{:.1}T", g / 1024.0)
    } else {
        format!("{g:.1}G")
    }
}

/// Unix seconds to `YYYY-MM-DD` (proleptic Gregorian, days-from-civil inverse).
fn fmt_date(secs: u64) -> String {
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn profile_label(p: &Profile) -> String {
    if p.total == 0 {
        return p.name.clone();
    }
    let used = fmt_bytes(p.upload + p.download);
    let expire = if p.expire > 0 {
        format!(" · {}", fmt_date(p.expire))
    } else {
        String::new()
    };
    format!("{}  ({used}/{}{expire})", p.name, fmt_bytes(p.total))
}

fn subscription_menu(menu: &Menu, snap: &Snapshot) {
    let sub = Submenu::new("订阅", true);
    for p in &snap.profiles {
        let on = snap.active.as_deref() == Some(p.id.as_str());
        let _ = sub.append(&CheckMenuItem::with_id(
            format!("sub:{}", p.id),
            profile_label(p),
            true,
            on,
            None,
        ));
    }
    let _ = sub.append(&PredefinedMenuItem::separator());
    let _ = sub.append(&MenuItem::with_id("add", "从剪贴板添加订阅", true, None));
    let has_active = snap.active.is_some();
    let update_id = format!("upd:{}", snap.active.clone().unwrap_or_default());
    let _ = sub.append(&MenuItem::with_id(
        update_id,
        "更新当前订阅",
        has_active,
        None,
    ));
    let delete = Submenu::new("删除订阅", !snap.profiles.is_empty());
    for p in &snap.profiles {
        let _ = delete.append(&MenuItem::with_id(
            format!("del:{}", p.id),
            &p.name,
            true,
            None,
        ));
    }
    let _ = sub.append(&delete);
    let _ = menu.append(&sub);
}

fn last_delay(proxies: &Value, name: &str) -> Option<u64> {
    let history = proxies[name]["history"].as_array()?;
    history.last()?["delay"].as_u64().filter(|d| *d > 0)
}

fn group_menu(all: &Value, name: &str) -> Submenu {
    let group = &all[name];
    let now = group["now"].as_str().unwrap_or("");
    let selectable = group["type"].as_str() == Some("Selector");
    let sub = Submenu::new(format!("{name}  [{now}]"), true);
    let _ = sub.append(&MenuItem::with_id(
        format!("delay:{name}"),
        "测速",
        true,
        None,
    ));
    let _ = sub.append(&PredefinedMenuItem::separator());
    for node in group["all"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let label = match last_delay(all, node) {
            Some(ms) => format!("{node}\t{ms}ms"),
            None => node.to_string(),
        };
        let id = format!("node:{name}{SEP}{node}");
        let _ = sub.append(&CheckMenuItem::with_id(
            id,
            label,
            selectable,
            node == now,
            None,
        ));
    }
    sub
}

fn group_menus(menu: &Menu, proxies: &Value, mode: &str) {
    let all = &proxies["proxies"];
    let order = all["GLOBAL"]["all"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    for name in order.filter(|n| all[*n]["all"].is_array()) {
        let _ = menu.append(&group_menu(all, name));
    }
    if mode == "global" {
        let _ = menu.append(&group_menu(all, "GLOBAL"));
    }
}

fn mode_menu(menu: &Menu, mode: &str) {
    let sub = Submenu::new("模式", true);
    for (key, label) in [("rule", "规则"), ("global", "全局"), ("direct", "直连")] {
        let _ = sub.append(&CheckMenuItem::with_id(
            format!("mode:{key}"),
            label,
            true,
            mode == key,
            None,
        ));
    }
    let _ = menu.append(&sub);
}

fn kernel_menus(app: &App, menu: &Menu) {
    let api = app.api();
    let (Ok(cfg), Ok(proxies)) = (api.configs(), api.proxies()) else {
        return item(menu, "noop", "内核无响应", false);
    };
    let mode = cfg["mode"].as_str().unwrap_or("rule").to_lowercase();
    group_menus(menu, &proxies, &mode);
    let _ = menu.append(&PredefinedMenuItem::separator());
    mode_menu(menu, &mode);
}

pub fn build(app: &App) -> Menu {
    let menu = Menu::new();
    let Some(snap) = snapshot(app) else {
        item(&menu, "noop", "正在处理，请稍后再右键…", false);
        item(&menu, "quit", "退出", true);
        return menu;
    };
    subscription_menu(&menu, &snap);
    let _ = menu.append(&PredefinedMenuItem::separator());
    if snap.running {
        kernel_menus(app, &menu);
    } else {
        item(&menu, "noop", "内核未运行", false);
    }
    check(&menu, "tun", "TUN 模式", snap.tun);
    check(&menu, "sysproxy", "系统代理", snap.sysproxy);
    let _ = menu.append(&PredefinedMenuItem::separator());
    if dashboard_available() {
        item(&menu, "dash", "打开面板", true);
    }
    item(&menu, "log", "查看日志", true);
    item(&menu, "mihomo", "更新 mihomo", true);
    item(
        &menu,
        "upgrade",
        &format!("检查更新 (当前 v{})", env!("CARGO_PKG_VERSION")),
        true,
    );
    check(&menu, "silent", "静默启动", !snap.show_dashboard);
    check(&menu, "auto", "开机自启", autostart::enabled());
    item(&menu, "quit", "退出", true);
    menu
}

/// Runs `job` off the UI thread and reports its outcome.
fn task(
    app: &Arc<App>,
    job: impl FnOnce(&App) -> crate::state::Res<Option<String>> + Send + 'static,
) {
    let app = Arc::clone(app);
    thread::spawn(move || {
        match job(&app) {
            Ok(Some(text)) => ui::message(&text),
            Ok(None) => {}
            Err(e) => ui::message(&format!("操作失败：{e}")),
        }
        ui::refresh();
    });
}

fn open(target: &str) {
    let (verb, target) = (ui::wide("open"), ui::wide(target));
    let null = std::ptr::null();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            null,
            null,
            SW_SHOWNORMAL,
        )
    };
}

pub fn handle(app: &Arc<App>, id: &str) {
    let (kind, arg) = id.split_once(':').unwrap_or((id, ""));
    let arg = arg.to_string();
    match kind {
        "quit" => ui::quit(),
        "log" => open(&base_dir().join("mihomo.log").to_string_lossy()),
        "dash" => open_dashboard(app),
        "mihomo" => task(app, |a| a.update_mihomo().map(Some)),
        "upgrade" => task(app, |a| crate::update::run(a.lock().port)),
        "silent" => task(app, |a| a.toggle_silent().map(|_| None)),
        "mode" => task(app, move |a| a.api().set_mode(&arg).map(|_| None)),
        "node" => task(app, move |a| {
            let (group, node) = arg.split_once(SEP).ok_or("无效的节点")?;
            a.api().select(group, node).map(|_| None)
        }),
        "delay" => task(app, move |a| a.api().group_delay(&arg).map(|_| None)),
        "sub" => task(app, move |a| a.switch(&arg).map(|_| None)),
        "upd" => task(app, move |a| {
            a.update(&arg).map(|_| Some("订阅已更新".into()))
        }),
        "del" => task(app, move |a| a.delete(&arg).map(|_| None)),
        "add" => task(app, |a| a.add_from_clipboard().map(Some)),
        "tun" => task(app, |a| {
            let on = a.lock().state.tun;
            a.set_tun(!on).map(|_| None)
        }),
        "sysproxy" => task(app, |a| a.toggle_sysproxy().map(|_| None)),
        "auto" => task(app, |_| autostart::set(!autostart::enabled()).map(|_| None)),
        _ => {}
    }
}

fn dashboard_available() -> bool {
    base_dir().join("ui").join("index.html").is_file()
}

pub fn open_dashboard(app: &App) {
    if dashboard_available() {
        open(&dashboard_url(app));
    }
}

fn dashboard_url(app: &App) -> String {
    let secret = app.lock().state.secret.clone();
    let (host, port) = (crate::core::CTL_HOST, crate::core::CTL_PORT);
    let secret = crate::api::encode(&secret);
    format!("http://{host}:{port}/ui/?hostname={host}&port={port}&secret={secret}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dates() {
        assert_eq!(fmt_date(0), "1970-01-01");
        assert_eq!(fmt_date(951_782_400), "2000-02-29");
        assert_eq!(fmt_date(1_893_456_000), "2030-01-01");
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(fmt_bytes(1_610_612_736), "1.5G");
        assert_eq!(fmt_bytes(2 * 1024 * 1024 * 1024 * 1024), "2.0T");
    }
}
