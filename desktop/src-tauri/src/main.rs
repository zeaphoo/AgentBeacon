#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod prefs;
mod supervisor;

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use prefs::Prefs;
use supervisor::Supervisor;

#[tauri::command]
fn retry_service(app: AppHandle) {
    if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
        supervisor.restart(&app);
    }
}

fn main() {
    supervisor::install_signal_handlers();

    tauri::Builder::default()
        // 单实例必须最先注册：第二次启动只唤起已有窗口，不再派生服务。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.iter().any(|arg| arg == "--hidden") {
                return;
            }
            if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                supervisor.open_console(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![retry_service])
        .setup(|app| {
            let handle = app.handle().clone();
            let hidden = std::env::args().any(|arg| arg == "--hidden");
            let prefs = Prefs::load(&handle);
            let port = config::service_port();
            let log_path = handle
                .path()
                .app_data_dir()
                .map(|dir| {
                    let _ = std::fs::create_dir_all(&dir);
                    dir.join("agent-beacon.log")
                })
                .unwrap_or_else(|_| std::path::PathBuf::from("agent-beacon.log"));

            let supervisor = Arc::new(Supervisor::new(port, hidden, prefs.clone(), log_path));
            app.manage(Arc::clone(&supervisor));

            let manager = handle.autolaunch();
            let system_autostart = manager.is_enabled().unwrap_or(prefs.autostart);
            if system_autostart != prefs.autostart {
                let _ = supervisor.set_autostart(&handle, prefs.autostart);
            }
            let actual_autostart = manager.is_enabled().unwrap_or(prefs.autostart);

            let open = MenuItem::with_id(app, "open", "打开控制台", true, None::<&str>)?;
            let start = MenuItem::with_id(app, "start", "启动服务", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "停止服务", true, None::<&str>)?;
            let off = MenuItem::with_id(app, "off", "关闭灯效", true, None::<&str>)?;
            let autostart = CheckMenuItem::with_id(
                app,
                "autostart",
                "开机自启动",
                true,
                actual_autostart,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &open,
                    &start,
                    &stop,
                    &PredefinedMenuItem::separator(app)?,
                    &off,
                    &PredefinedMenuItem::separator(app)?,
                    &autostart,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?;

            let tray_supervisor = Arc::clone(&supervisor);
            let tray_autostart = autostart.clone();
            TrayIconBuilder::with_id("agent-beacon")
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("AgentBeacon")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => tray_supervisor.open_console(app),
                    "start" => tray_supervisor.restart(app),
                    "stop" => tray_supervisor.stop(app),
                    "off" => tray_supervisor.lights_off(),
                    "autostart" => {
                        let intent = tray_autostart.is_checked().unwrap_or(false);
                        if tray_supervisor.set_autostart(app, intent).is_err() {
                            let _ = tray_autostart.set_checked(!intent);
                        }
                    }
                    "quit" => {
                        tray_supervisor.shutdown();
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            supervisor.start(&handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| match event {
            RunEvent::ExitRequested { code, api, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => {
                if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                    supervisor.shutdown();
                }
            }
            _ => {}
        });
}
