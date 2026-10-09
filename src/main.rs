#![windows_subsystem = "windows"]

mod api;
mod app;
mod autostart;
mod clipboard;
mod core;
#[cfg(test)]
mod e2e_tests;
mod menu;
mod state;
mod subscription;
mod sysproxy;
mod ui;

use app::App;
use std::{
    cell::RefCell,
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    thread,
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, menu::MenuEvent,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG, TranslateMessage, WM_APP},
};

const GRAY: [u8; 3] = [136, 136, 136];
const GREEN: [u8; 3] = [76, 175, 80];
const BLUE: [u8; 3] = [33, 150, 243];

thread_local! {
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

fn already_running() -> bool {
    let name = ui::wide("Local\\ClashLiteSingleton");
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

/// Syncs icon colour and tooltip with the core state (skipped while busy).
fn sync_tray(app: &App) {
    let Some(mut g) = app.try_lock() else { return };
    let (color, tip) = match (g.core.alive(), g.state.tun) {
        (false, _) => (GRAY, "Clash Lite · 未运行"),
        (true, true) => (BLUE, "Clash Lite · TUN"),
        (true, false) => (GREEN, "Clash Lite · 运行中"),
    };
    drop(g);
    TRAY.with(|t| {
        if let Some(tray) = t.borrow().as_ref() {
            let _ = tray.set_icon(Some(ui::icon(color)));
            let _ = tray.set_tooltip(Some(tip));
        }
    });
}

fn install_handlers(app: &Arc<App>) {
    let a = Arc::clone(app);
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let TrayIconEvent::Click {
            button: MouseButton::Right,
            button_state: MouseButtonState::Up,
            ..
        } = event
        else {
            return;
        };
        // The menu is built right before the library pops it up, so it is always fresh.
        let fresh = menu::build(&a);
        TRAY.with(|t| {
            if let Some(tray) = t.borrow().as_ref() {
                tray.set_menu(Some(Box::new(fresh)));
            }
        });
        sync_tray(&a);
    }));
    let a = Arc::clone(app);
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        menu::handle(&a, event.id.as_ref())
    }));
}

fn spawn_workers(app: &Arc<App>, wake: mpsc::Receiver<()>) {
    let a = Arc::clone(app);
    thread::spawn(move || {
        match a.start() {
            Err(e) => ui::message(&format!("启动失败：{e}")),
            Ok(()) if a.lock().state.show_dashboard => menu::open_dashboard(&a),
            Ok(()) => {}
        }
        ui::refresh();
        a.update_due();
        loop {
            match wake.recv_timeout(a.next_due()) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => a.update_due(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn run_message_loop(app: &App) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        if msg.message == WM_APP && msg.hwnd.is_null() {
            sync_tray(app);
            continue;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn main() {
    if already_running() {
        return;
    }
    ui::mark_main_thread();
    let (tx, rx) = mpsc::channel();
    let app = Arc::new(App::new(tx));
    let tray = TrayIconBuilder::new()
        .with_icon(ui::icon(GRAY))
        .with_tooltip("Clash Lite")
        .build()
        .expect("create tray icon");
    TRAY.with(|t| *t.borrow_mut() = Some(tray));
    install_handlers(&app);
    spawn_workers(&app, rx);
    run_message_loop(&app);
    TRAY.with(|t| t.borrow_mut().take());
    app.shutdown();
}
