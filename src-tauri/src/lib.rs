use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotchInfo {
    pub has_notch: bool,
    pub notch_width: f64,
    pub menu_bar_height: f64,
    pub screen_width: f64,
    pub left_area_width: f64,
    pub right_area_width: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatusBarApp {
    pub pid: i32,
    pub name: String,
    pub bundle_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BarloConfig {
    pub enabled: bool,
    pub barlo_bar_visible: bool,
    pub hidden_app_pids: Vec<i32>,
    pub auto_hide_for_notch: bool,
}

impl Default for BarloConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            barlo_bar_visible: false,
            hidden_app_pids: Vec::new(),
            auto_hide_for_notch: true,
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct NSEdgeInsets {
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct NSSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }

    pub fn get_notch_info() -> NotchInfo {
        unsafe {
            let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];

            // safeAreaInsets: top > 0 means notch present (macOS 12+)
            let insets: NSEdgeInsets = msg_send![screen, safeAreaInsets];

            // Screen frame (full screen including menu bar)
            let frame: NSRect = msg_send![screen, frame];

            // Visible frame excludes menu bar and dock
            let visible_frame: NSRect = msg_send![screen, visibleFrame];

            // Menu bar height
            let menu_bar_height =
                frame.size.height - visible_frame.size.height - visible_frame.origin.y;

            // Areas to left and right of notch
            // auxiliaryTopLeftArea / auxiliaryTopRightArea available on macOS 12+
            let aux_left: NSRect = msg_send![screen, auxiliaryTopLeftArea];
            let aux_right: NSRect = msg_send![screen, auxiliaryTopRightArea];

            let has_notch = insets.top > 0.0 || aux_left.size.width > 0.0;

            let notch_width = if has_notch && aux_right.origin.x > 0.0 {
                aux_right.origin.x - (aux_left.origin.x + aux_left.size.width)
            } else if has_notch {
                // Fallback estimate: MacBook Pro notch ~200pts wide
                200.0
            } else {
                0.0
            };

            NotchInfo {
                has_notch,
                notch_width,
                menu_bar_height,
                screen_width: frame.size.width,
                left_area_width: aux_left.size.width,
                right_area_width: aux_right.size.width,
            }
        }
    }

    pub fn get_status_bar_apps() -> Vec<StatusBarApp> {
        unsafe {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            let apps: *mut Object = msg_send![workspace, runningApplications];
            let count: usize = msg_send![apps, count];

            let mut result = Vec::new();

            for i in 0..count {
                let app: *mut Object = msg_send![apps, objectAtIndex: i];

                // activationPolicy 1 = NSApplicationActivationPolicyAccessory (menu bar apps)
                // activationPolicy 2 = NSApplicationActivationPolicyProhibited (background)
                // activationPolicy 0 = NSApplicationActivationPolicyRegular (normal apps)
                let policy: i64 = msg_send![app, activationPolicy];

                // Include accessory apps (menu bar utilities) and regular apps that may have status items
                if policy == 1 || policy == 0 {
                    let pid: i32 = msg_send![app, processIdentifier];
                    let name_obj: *mut Object = msg_send![app, localizedName];
                    let bundle_id_obj: *mut Object = msg_send![app, bundleIdentifier];

                    let name: &str = if name_obj.is_null() {
                        "Unknown"
                    } else {
                        let bytes: *const u8 = msg_send![name_obj, UTF8String];
                        if bytes.is_null() {
                            "Unknown"
                        } else {
                            std::ffi::CStr::from_ptr(bytes as *const i8)
                                .to_str()
                                .unwrap_or("Unknown")
                        }
                    };

                    let bundle_id: &str = if bundle_id_obj.is_null() {
                        ""
                    } else {
                        let bytes: *const u8 = msg_send![bundle_id_obj, UTF8String];
                        if bytes.is_null() {
                            ""
                        } else {
                            std::ffi::CStr::from_ptr(bytes as *const i8)
                                .to_str()
                                .unwrap_or("")
                        }
                    };

                    result.push(StatusBarApp {
                        pid,
                        name: name.to_string(),
                        bundle_id: bundle_id.to_string(),
                    });
                }
            }

            result
        }
    }

    pub fn check_accessibility_permission() -> bool {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }

        unsafe { AXIsProcessTrusted() }
    }

    pub fn request_accessibility_permission() {
        // Open System Settings > Privacy > Accessibility
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

#[tauri::command]
fn get_notch_info() -> NotchInfo {
    #[cfg(target_os = "macos")]
    {
        macos::get_notch_info()
    }
    #[cfg(not(target_os = "macos"))]
    {
        NotchInfo {
            has_notch: false,
            notch_width: 0.0,
            menu_bar_height: 24.0,
            screen_width: 1920.0,
            left_area_width: 0.0,
            right_area_width: 0.0,
        }
    }
}

#[tauri::command]
fn get_status_bar_apps() -> Vec<StatusBarApp> {
    #[cfg(target_os = "macos")]
    {
        macos::get_status_bar_apps()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[tauri::command]
fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::check_accessibility_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[tauri::command]
fn request_accessibility() {
    #[cfg(target_os = "macos")]
    {
        macos::request_accessibility_permission()
    }
}

#[tauri::command]
async fn show_settings(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
async fn toggle_barlo_bar(visible: bool, app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("barlo-bar") {
        if visible {
            dock_barlo_bar(&window);
            let _ = window.show();
        } else {
            let _ = window.hide();
        }
    }
}

/// Pins the Barlo Bar window directly below the menu bar, right-aligned at 50% width.
fn dock_barlo_bar(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let info = macos::get_notch_info();
        let bar_width = info.screen_width / 2.0;
        let bar_x = info.screen_width - bar_width;
        let _ = window.set_size(tauri::LogicalSize::new(bar_width, 40.0));
        let _ = window.set_position(tauri::LogicalPosition::new(bar_x, info.menu_bar_height));
    }
}

#[tauri::command]
async fn position_barlo_bar(app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Some(window) = app.get_webview_window("barlo-bar") {
            let notch_info = macos::get_notch_info();
            let _ = window.set_position(tauri::PhysicalPosition::new(
                0i32,
                notch_info.menu_bar_height as i32,
            ));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Run as menu bar app on macOS (no dock icon)
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            // barlo-bar is defined in tauri.conf.json with transparent: true.
            // Position it correctly based on the current screen layout.
            if let Some(barlo_bar) = app.get_webview_window("barlo-bar") {
                dock_barlo_bar(&barlo_bar);
            }

            // Build tray menu
            let settings_item =
                MenuItem::with_id(app, "settings", "Barlo Settings...", true, None::<&str>)?;
            let barlo_bar_item = MenuItem::with_id(
                app,
                "toggle-barlo-bar",
                "Show Barlo Bar",
                true,
                None::<&str>,
            )?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Barlo", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[&settings_item, &barlo_bar_item, &separator, &quit_item],
            )?;

            // Create tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "settings" => {
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "toggle-barlo-bar" => {
                        if let Some(window) = app.get_webview_window("barlo-bar") {
                            let visible = window.is_visible().unwrap_or(false);
                            if visible {
                                let _ = window.hide();
                            } else {
                                dock_barlo_bar(&window);
                                let _ = window.show();
                            }
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_notch_info,
            get_status_bar_apps,
            check_accessibility,
            request_accessibility,
            show_settings,
            toggle_barlo_bar,
            position_barlo_bar,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
