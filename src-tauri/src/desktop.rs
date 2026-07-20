use crate::api::AppState;
use tauri::{
    App, AppHandle, Emitter, Manager, Window, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const OPEN_MAIN_ID: &str = "open-main";
const NEW_POWERSHELL_ID: &str = "new-powershell";
const QUIT_ID: &str = "quit";

pub fn configure(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let open_main = MenuItem::with_id(app, OPEN_MAIN_ID, "Open EnvManager", true, None::<&str>)?;
    let new_powershell =
        MenuItem::with_id(app, NEW_POWERSHELL_ID, "New PowerShell", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_main, &new_powershell, &quit])?;

    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("EnvManager")
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = toggle_quick_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_MAIN_ID => {
                let _ = show_main_window(app);
            }
            NEW_POWERSHELL_ID => {
                if let Err(error) = app.state::<AppState>().launch_powershell() {
                    let _ = app.emit("desktop-error", error);
                }
            }
            QUIT_ID => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "quick" {
            api.prevent_close();
            let _ = window.hide();
        } else if window.label() == "main" {
            window.app_handle().exit(0);
        }
    }
}

fn toggle_quick_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("quick") {
        if window.is_visible()? {
            window.hide()?;
        } else {
            window.show()?;
            window.set_focus()?;
        }
    }
    Ok(())
}

fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
    }
    Ok(())
}
