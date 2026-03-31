use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

// ── Global state ───────────────────────────────────────────────────────────────
static ICONS_HIDDEN: AtomicBool = AtomicBool::new(false);
static DOTS_ITEM_PTR: AtomicUsize = AtomicUsize::new(0);
static DOTS_HANDLER_PTR: AtomicUsize = AtomicUsize::new(0);
static ANCHOR_ITEM_PTR: AtomicUsize = AtomicUsize::new(0);

// Strategie A (älteres macOS): View direkt in _statusBarWindow injizieren
static SB_WIN_PTR: AtomicUsize = AtomicUsize::new(0);
static OVERLAY_VIEW_PTR: AtomicUsize = AtomicUsize::new(0);

// Strategie C (macOS 15+): eigenes Fenster + Screenshot-Hintergrund
static OVERLAY_WIN_PTR: AtomicUsize = AtomicUsize::new(0);
static OVERLAY_IMG_VIEW_PTR: AtomicUsize = AtomicUsize::new(0);

// Wallpaper-Refresh: Observer-Objekt + laufender NSTimer + zuletzt gesehene URL
static WALLPAPER_OBSERVER_PTR: AtomicUsize = AtomicUsize::new(0);
static REFRESH_TIMER_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_WALLPAPER_URL_PTR: AtomicUsize = AtomicUsize::new(0); // retained NSString*

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

// ─────────────────────────────────────────────────────────────────────────────

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
pub struct StatusItemInfo {
    pub icon_base64: String,
    pub click_x: f64,
    pub click_y: f64,
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
    use objc::declare::ClassDecl;
    use objc::runtime::{Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSEdgeInsets {
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }

    // ── C-Bindings ─────────────────────────────────────────────────────────
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        /// Prüft ob Screen Recording erlaubt ist (kein Dialog).
        fn CGPreflightScreenCaptureAccess() -> bool;
        /// Fordert Screen Recording an — zeigt System-Dialog beim ersten Mal.
        fn CGRequestScreenCaptureAccess() -> bool;

        /// Erstellt ein CGImage von allem unterhalb von `window_id` auf dem Bildschirm.
        /// listOption = 8 (kCGWindowListOptionOnScreenBelowWindow)
        /// imageOption = 0 (kCGWindowImageDefault)
        fn CGWindowListCreateImage(
            screen_bounds: NSRect,
            list_option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *mut std::ffi::c_void; // CGImageRef

        fn CGImageRelease(image: *mut std::ffi::c_void);

        fn CGEventCreateMouseEvent(
            source: *const std::ffi::c_void,
            mouse_type: u32,
            cursor_position: NSPoint,
            mouse_button: u32,
        ) -> *mut std::ffi::c_void;
        fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
        fn CFRelease(cf: *mut std::ffi::c_void);

        /// Listet alle On-Screen-Fenster als CFArrayRef von CFDictionaryRef auf.
        /// option = 1 (kCGWindowListOptionOnScreenOnly)
        fn CGWindowListCopyWindowInfo(
            option: u32,
            relative_to_window: u32,
        ) -> *mut std::ffi::c_void; // CFArrayRef (toll-free bridged mit NSArray)
    }

    // ── Public API ─────────────────────────────────────────────────────────

    pub fn check_screen_recording() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    pub fn request_screen_recording() {
        unsafe {
            CGRequestScreenCaptureAccess();
        }
    }

    pub fn get_notch_info() -> NotchInfo {
        unsafe {
            let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
            let insets: NSEdgeInsets = msg_send![screen, safeAreaInsets];
            let frame: NSRect = msg_send![screen, frame];
            let visible_frame: NSRect = msg_send![screen, visibleFrame];
            let menu_bar_height =
                frame.size.height - visible_frame.size.height - visible_frame.origin.y;
            let aux_left: NSRect = msg_send![screen, auxiliaryTopLeftArea];
            let aux_right: NSRect = msg_send![screen, auxiliaryTopRightArea];
            let has_notch = insets.top > 0.0 || aux_left.size.width > 0.0;
            let notch_width = if has_notch && aux_right.origin.x > 0.0 {
                aux_right.origin.x - (aux_left.origin.x + aux_left.size.width)
            } else if has_notch {
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
                let policy: i64 = msg_send![app, activationPolicy];
                if policy == 1 || policy == 0 {
                    let pid: i32 = msg_send![app, processIdentifier];
                    let name_obj: *mut Object = msg_send![app, localizedName];
                    let bundle_id_obj: *mut Object = msg_send![app, bundleIdentifier];
                    let name = ns_str(name_obj).unwrap_or_else(|| "Unknown".into());
                    let bundle_id = ns_str(bundle_id_obj).unwrap_or_default();
                    result.push(StatusBarApp {
                        pid,
                        name,
                        bundle_id,
                    });
                }
            }
            result
        }
    }

    unsafe fn ns_str(obj: *mut Object) -> Option<String> {
        if obj.is_null() {
            return None;
        }
        let bytes: *const u8 = msg_send![obj, UTF8String];
        if bytes.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(bytes as *const i8)
            .to_str()
            .ok()
            .map(|s| s.to_string())
    }

    pub fn check_accessibility_permission() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn request_accessibility_permission() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    // ── Position-Hilfe ─────────────────────────────────────────────────────

    unsafe fn status_item_frame(item_ptr: usize) -> Option<NSRect> {
        if item_ptr == 0 {
            return None;
        }
        let item = item_ptr as *mut Object;
        let button: *mut Object = msg_send![item, button];
        if button.is_null() {
            return None;
        }
        let win: *mut Object = msg_send![button, window];
        if win.is_null() {
            return None;
        }
        let frame: NSRect = msg_send![win, frame];
        Some(frame)
    }

    /// Gibt (left_screen, right_screen, y_screen, height) in Cocoa-Koordinaten zurück.
    unsafe fn overlay_bounds() -> Option<(f64, f64, f64, f64)> {
        let dots_frame = status_item_frame(DOTS_ITEM_PTR.load(Ordering::SeqCst))?;
        let anchor_frame = status_item_frame(ANCHOR_ITEM_PTR.load(Ordering::SeqCst));

        let right_x = dots_frame.origin.x;
        let left_x = match anchor_frame {
            Some(f) => f.origin.x + f.size.width,
            None => {
                let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
                let aux: NSRect = msg_send![screen, auxiliaryTopRightArea];
                if aux.size.width > 0.0 {
                    aux.origin.x
                } else {
                    return None;
                }
            }
        };

        if left_x >= right_x {
            eprintln!(
                "[Barlo] overlay_bounds: left({}) >= right({})",
                left_x, right_x
            );
            return None;
        }

        let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
        let scr: NSRect = msg_send![screen, frame];
        let vis: NSRect = msg_send![screen, visibleFrame];
        let mb_h = scr.size.height - vis.size.height - vis.origin.y;
        let y = scr.size.height - mb_h;

        eprintln!(
            "[Barlo] bounds: left={:.0} right={:.0} y={:.0} h={:.0} width={:.0}",
            left_x,
            right_x,
            y,
            mb_h,
            right_x - left_x
        );
        Some((left_x, right_x, y, mb_h))
    }

    // ── Strategie A: _statusBarWindow ──────────────────────────────────────

    unsafe fn find_status_bar_window() -> *mut Object {
        // parentWindow (public API)
        let dots_ptr = DOTS_ITEM_PTR.load(Ordering::SeqCst);
        if dots_ptr != 0 {
            let item = dots_ptr as *mut Object;
            let button: *mut Object = msg_send![item, button];
            if !button.is_null() {
                let item_win: *mut Object = msg_send![button, window];
                if !item_win.is_null() {
                    let parent: *mut Object = msg_send![item_win, parentWindow];
                    if !parent.is_null() {
                        let f: NSRect = msg_send![parent, frame];
                        eprintln!(
                            "[Barlo] A: parentWindow w={:.0} h={:.0}",
                            f.size.width, f.size.height
                        );
                        return parent;
                    }
                }
            }
        }
        // _statusBarWindow (private API, macOS < 15)
        let status_bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
        let sel = objc::runtime::Sel::register("_statusBarWindow");
        let responds: bool = msg_send![status_bar, respondsToSelector: sel];
        if responds {
            let win: *mut Object = msg_send![status_bar, _statusBarWindow];
            if !win.is_null() {
                let f: NSRect = msg_send![win, frame];
                eprintln!(
                    "[Barlo] A: _statusBarWindow w={:.0} h={:.0}",
                    f.size.width, f.size.height
                );
                return win;
            }
        }
        std::ptr::null_mut()
    }

    // ── Strategie C: Screenshot-Overlay (macOS 15+) ────────────────────────

    /// Erstellt das Overlay-Fenster (NSWindow + NSImageView) einmalig.
    unsafe fn create_strategy_c_overlay() {
        if OVERLAY_WIN_PTR.load(Ordering::SeqCst) != 0 {
            return;
        }

        let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
        let scr: NSRect = msg_send![screen, frame];
        let vis: NSRect = msg_send![screen, visibleFrame];
        let mb_h = scr.size.height - vis.size.height - vis.origin.y;

        let initial = NSRect {
            origin: NSPoint {
                x: 0.0,
                y: scr.size.height - mb_h,
            },
            size: NSSize {
                width: 1.0,
                height: mb_h,
            },
        };

        let win: *mut Object = msg_send![class!(NSWindow), alloc];
        let win: *mut Object = msg_send![win,
            initWithContentRect: initial
            styleMask: 0u64
            backing: 2u64
            defer: objc::runtime::NO
        ];
        if win.is_null() {
            return;
        }

        // Level 26 = über allen Status-Items (Level 25)
        let _: () = msg_send![win, setLevel: 26i64];
        let _: () = msg_send![win, setOpaque: objc::runtime::YES];
        let _: () = msg_send![win, setIgnoresMouseEvents: objc::runtime::NO];
        // Auf allen Spaces sichtbar, kein Cycling
        let _: () = msg_send![win, setCollectionBehavior: 4105u64];

        // NSImageView als ContentView
        let img_view: *mut Object = msg_send![class!(NSImageView), alloc];
        let img_view: *mut Object = msg_send![img_view, initWithFrame: initial];
        // NSImageScaleAxesIndependently = 2 → streckt Bild auf Fenstergröße
        let _: () = msg_send![img_view, setImageScaling: 2i64];
        let _: () = msg_send![win, setContentView: img_view];

        let _: () = msg_send![win, retain];
        let _: () = msg_send![img_view, retain];
        OVERLAY_WIN_PTR.store(win as usize, Ordering::SeqCst);
        OVERLAY_IMG_VIEW_PTR.store(img_view as usize, Ordering::SeqCst);
        eprintln!("[Barlo] C: Overlay-Fenster erstellt (Level 26)");
    }

    /// Findet das unterste Level-25-Fenster (NSStatusBarWindowLevel) als Referenz fuer
    /// CGWindowListCreateImage. Auf macOS 15 liefert [NSWindow windowNumber] fuer Status-Items
    /// keinen gueltigen CGWindowID, daher Enumeration via CGWindowListCopyWindowInfo.
    /// Das unterste Level-25-Fenster als Referenz → "darunter" sind nur noch Level-<25-Fenster
    /// = reiner Menueleisten-Hintergrund ohne Status-Icons.
    unsafe fn find_reference_window_id() -> u32 {
        // Erst direkten windowNumber versuchen (funktioniert auf aelteren macOS)
        let dots_ptr = DOTS_ITEM_PTR.load(Ordering::SeqCst);
        if dots_ptr != 0 {
            let item = dots_ptr as *mut Object;
            let button: *mut Object = msg_send![item, button];
            if !button.is_null() {
                let win: *mut Object = msg_send![button, window];
                if !win.is_null() {
                    let num: i64 = msg_send![win, windowNumber];
                    let num_u32 = num as u32;
                    if num > 0 && num_u32 > 0 {
                        return num_u32;
                    }
                }
            }
        }

        // Fallback: unterstes Level-25-Fenster aus der globalen Fensterliste suchen.
        // kCGWindowListOptionOnScreenOnly = 1
        let arr = CGWindowListCopyWindowInfo(1u32, 0u32) as *mut Object;
        if arr.is_null() {
            return 0;
        }

        let count: usize = msg_send![arr, count];
        let key_layer = nsstring_static("kCGWindowLayer");
        let key_num = nsstring_static("kCGWindowNumber");

        // CGWindowListCopyWindowInfo: Front→Back. Letztes Level-25 = unterstes in Z-Order.
        let mut last_level25_id: u32 = 0;
        for i in 0..count {
            let info: *mut Object = msg_send![arr, objectAtIndex: i];
            let layer_obj: *mut Object = msg_send![info, objectForKey: key_layer];
            if layer_obj.is_null() {
                continue;
            }
            let layer: i32 = msg_send![layer_obj, intValue];
            if layer != 25 {
                continue;
            }
            let num_obj: *mut Object = msg_send![info, objectForKey: key_num];
            if num_obj.is_null() {
                continue;
            }
            let wid: u32 = msg_send![num_obj, unsignedIntValue];
            last_level25_id = wid;
        }

        let _: () = msg_send![arr, release];
        last_level25_id
    }

    /// Erstellt einen temporaeren NSString aus einem Rust-String (kein \0 noetig).
    unsafe fn nsstring_static(s: &str) -> *mut Object {
        // CString fuegt den null-Terminator hinzu; NSString kopiert die Bytes sofort.
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()]
    }

    /// Fotografiert alles unterhalb des Dots-Fensters (= Menueleisten-Hintergrund ohne Icons).
    /// Gibt CGImageRef zurueck (muss mit CGImageRelease freigegeben werden).
    unsafe fn capture_background_below_dots(cocoa_rect: NSRect) -> *mut std::ffi::c_void {
        let window_num_u32 = find_reference_window_id();
        if window_num_u32 == 0 {
            eprintln!("[Barlo] capture: keine Level-25-Referenz (Screen Recording benoetigt?)");
            return std::ptr::null_mut();
        }

        let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
        let scr: NSRect = msg_send![screen, frame];

        // Koordinatenumrechnung: Cocoa (Y-unten) → CoreGraphics (Y-oben)
        let cg_rect = NSRect {
            origin: NSPoint {
                x: cocoa_rect.origin.x,
                y: scr.size.height - cocoa_rect.origin.y - cocoa_rect.size.height,
            },
            size: cocoa_rect.size,
        };

        // kCGWindowListOptionOnScreenBelowWindow = (1<<2) = 4, kCGWindowImageDefault = 0
        CGWindowListCreateImage(cg_rect, 4u32, window_num_u32, 0u32)
    }

    /// Macht einen Screenshot der versteckten Zone MIT den Icons (vor dem Verstecken).
    /// Gibt das Bild als Base64-PNG zurück, damit die Barlo Bar es anzeigen kann.
    unsafe fn capture_icon_strip_base64() -> Option<String> {
        let (left, right, y, h) = overlay_bounds()?;
        let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
        let scr: NSRect = msg_send![screen, frame];
        // Cocoa → CG Koordinaten
        let cg_rect = NSRect {
            origin: NSPoint {
                x: left,
                y: scr.size.height - y - h,
            },
            size: NSSize {
                width: right - left,
                height: h,
            },
        };
        // kCGWindowListOptionOnScreenOnly = 1, kCGNullWindowID = 0 → alles sichtbare
        let cg_image = CGWindowListCreateImage(cg_rect, 1u32, 0u32, 0u32);
        if cg_image.is_null() {
            return None;
        }
        let logical_size = NSSize {
            width: right - left,
            height: h,
        };
        let rep: *mut Object = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut Object = msg_send![rep, initWithCGImage: cg_image];
        let _: () = msg_send![rep, setSize: logical_size];
        CGImageRelease(cg_image);
        // NSBitmapImageFileTypePNG = 4
        let png_data: *mut Object = msg_send![rep,
            representationUsingType: 4u64
            properties: std::ptr::null_mut::<Object>()
        ];
        let _: () = msg_send![rep, release];
        if png_data.is_null() {
            return None;
        }
        // NSData hat eingebautes Base64 — kein externer Crate nötig
        let b64: *mut Object = msg_send![png_data, base64EncodedStringWithOptions: 0u64];
        ns_str(b64)
    }

    // ── Overlay initialisieren ─────────────────────────────────────────────

    fn ensure_overlay_created() -> bool {
        // Bereits erstellt?
        if OVERLAY_VIEW_PTR.load(Ordering::SeqCst) != 0
            || OVERLAY_WIN_PTR.load(Ordering::SeqCst) != 0
        {
            return true;
        }
        unsafe {
            // Strategie A: Status-Bar-Fenster injizieren
            let sb_win = find_status_bar_window();
            if !sb_win.is_null() {
                let sb_frame: NSRect = msg_send![sb_win, frame];
                SB_WIN_PTR.store(sb_win as usize, Ordering::SeqCst);
                let content_view: *mut Object = msg_send![sb_win, contentView];
                if !content_view.is_null() {
                    let r = NSRect {
                        origin: NSPoint { x: 0.0, y: 0.0 },
                        size: NSSize {
                            width: 1.0,
                            height: sb_frame.size.height,
                        },
                    };
                    let vev: *mut Object = msg_send![class!(NSVisualEffectView), alloc];
                    let vev: *mut Object = msg_send![vev, initWithFrame: r];
                    let _: () = msg_send![vev, setMaterial: 5i64];
                    let _: () = msg_send![vev, setBlendingMode: 1i64];
                    let _: () = msg_send![vev, setState: 1i64];
                    let _: () = msg_send![vev, setHidden: objc::runtime::YES];
                    let _: () = msg_send![content_view, addSubview: vev];
                    let _: () = msg_send![vev, retain];
                    OVERLAY_VIEW_PTR.store(vev as usize, Ordering::SeqCst);
                    eprintln!("[Barlo] A: View in Status-Bar-Fenster injiziert");
                    return true;
                }
            }
            // Strategie C: eigenes Fenster + Screenshot
            create_strategy_c_overlay();
            OVERLAY_WIN_PTR.load(Ordering::SeqCst) != 0
        }
    }

    // ── Show / Hide ────────────────────────────────────────────────────────

    pub fn show_overlay() {
        if !ensure_overlay_created() {
            return;
        }

        unsafe {
            let (left, right, y, h) = match overlay_bounds() {
                Some(b) => b,
                None => return,
            };

            // Strategie A
            let view_ptr = OVERLAY_VIEW_PTR.load(Ordering::SeqCst);
            if view_ptr != 0 {
                let sb_win = SB_WIN_PTR.load(Ordering::SeqCst) as *mut Object;
                let sb_frame: NSRect = msg_send![sb_win, frame];
                let view_frame = NSRect {
                    origin: NSPoint {
                        x: left - sb_frame.origin.x,
                        y: 0.0,
                    },
                    size: NSSize {
                        width: right - left,
                        height: h,
                    },
                };
                let vev = view_ptr as *mut Object;
                let _: () = msg_send![vev, setFrame: view_frame];
                let _: () = msg_send![vev, setHidden: objc::runtime::NO];
                return;
            }

            // Strategie C
            let win_ptr = OVERLAY_WIN_PTR.load(Ordering::SeqCst);
            let img_view_ptr = OVERLAY_IMG_VIEW_PTR.load(Ordering::SeqCst);
            if win_ptr == 0 || img_view_ptr == 0 {
                return;
            }

            let win = win_ptr as *mut Object;
            let img_view = img_view_ptr as *mut Object;

            // Screenshot der Menüleiste OHNE Icons (alles unterhalb des Dots-Fensters)
            let cocoa_rect = NSRect {
                origin: NSPoint { x: left, y },
                size: NSSize {
                    width: right - left,
                    height: h,
                },
            };

            let cg_image = capture_background_below_dots(cocoa_rect);
            if !cg_image.is_null() {
                // NSBitmapImageRep → setSize (logische Punktgroesse) → NSImage.
                // So kennt NSImage die echte DPI und rendert pixelgenau auf Retina.
                let logical_size = NSSize {
                    width: right - left,
                    height: h,
                };
                let rep: *mut Object = msg_send![class!(NSBitmapImageRep), alloc];
                let rep: *mut Object = msg_send![rep, initWithCGImage: cg_image];
                let _: () = msg_send![rep, setSize: logical_size];
                let ns_img: *mut Object = msg_send![class!(NSImage), alloc];
                let ns_img: *mut Object = msg_send![ns_img, initWithSize: logical_size];
                if !ns_img.is_null() && !rep.is_null() {
                    let _: () = msg_send![ns_img, addRepresentation: rep];
                    let _: () = msg_send![img_view, setImage: ns_img];
                }
                // release our +1 — NSImageView/NSImage retain what they need
                if !ns_img.is_null() {
                    let _: () = msg_send![ns_img, release];
                }
                if !rep.is_null() {
                    let _: () = msg_send![rep, release];
                }
                CGImageRelease(cg_image);
            } else {
                // Fallback (Screen Recording nicht gewaehrt): Systemfarbe
                let color: *mut Object = msg_send![class!(NSColor), windowBackgroundColor];
                let _: () = msg_send![win, setBackgroundColor: color];
                let _: () = msg_send![img_view, setImage: std::ptr::null_mut::<Object>()];
            }

            let win_frame = NSRect {
                origin: NSPoint { x: left, y },
                size: NSSize {
                    width: right - left,
                    height: h,
                },
            };
            let _: () = msg_send![win, setFrame: win_frame display: objc::runtime::YES];
            let _: () = msg_send![win, orderFrontRegardless];
        }
    }

    pub fn hide_overlay() {
        unsafe {
            // Strategie A
            let view_ptr = OVERLAY_VIEW_PTR.load(Ordering::SeqCst);
            if view_ptr != 0 {
                let vev = view_ptr as *mut Object;
                let _: () = msg_send![vev, setHidden: objc::runtime::YES];
                return;
            }
            // Strategie C
            let win_ptr = OVERLAY_WIN_PTR.load(Ordering::SeqCst);
            if win_ptr != 0 {
                let win = win_ptr as *mut Object;
                let _: () = msg_send![win, orderOut: std::ptr::null_mut::<Object>()];
            }
        }
    }

    // ── Anker-NSStatusItem ─────────────────────────────────────────────────

    pub fn create_anchor_status_item() {
        unsafe {
            let status_bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
            let item: *mut Object = msg_send![status_bar, statusItemWithLength: 8.0f64];
            if item.is_null() {
                return;
            }
            let button: *mut Object = msg_send![item, button];
            // U+2502 BOX DRAWINGS LIGHT VERTICAL = "│" als visueller Trenner
            let title: *mut Object = msg_send![class!(NSString),
                stringWithUTF8String: b"\xe2\x94\x82\0".as_ptr() as *const i8];
            let _: () = msg_send![button, setTitle: title];
            let _: () = msg_send![button, setBordered: objc::runtime::NO];
            // Kleiner Font damit "|" schlanker wirkt
            let font: *mut Object = msg_send![class!(NSFont),
                systemFontOfSize: 10.0f64];
            if !font.is_null() {
                let _: () = msg_send![button, setFont: font];
            }
            let _: () = msg_send![item, retain];
            ANCHOR_ITEM_PTR.store(item as usize, Ordering::SeqCst);
            eprintln!("[Barlo] Anchor-Item erstellt");
        }
    }

    // ── Button-Titel-Feedback ──────────────────────────────────────────────

    fn update_dots_title(hidden: bool) {
        unsafe {
            // Dots-Button: ⋯ wenn sichtbar, │ wenn versteckt
            let dots_ptr = DOTS_ITEM_PTR.load(Ordering::SeqCst);
            if dots_ptr != 0 {
                let item = dots_ptr as *mut Object;
                let button: *mut Object = msg_send![item, button];
                // ⋯ = \xe2\x8b\xaf, │ = \xe2\x94\x82
                let bytes: &[u8] = if hidden {
                    b"\xe2\x94\x82\0"
                } else {
                    b"\xe2\x8b\xaf\0"
                };
                let title: *mut Object = msg_send![class!(NSString),
                    stringWithUTF8String: bytes.as_ptr() as *const i8];
                let _: () = msg_send![button, setTitle: title];
            }
            // Anker: ausblenden wenn Icons versteckt, einblenden wenn sichtbar
            let anchor_ptr = ANCHOR_ITEM_PTR.load(Ordering::SeqCst);
            if anchor_ptr != 0 {
                let anchor = anchor_ptr as *mut Object;
                let anchor_button: *mut Object = msg_send![anchor, button];
                let hidden_val = if hidden {
                    objc::runtime::YES
                } else {
                    objc::runtime::NO
                };
                let _: () = msg_send![anchor_button, setHidden: hidden_val];
            }
        }
    }

    // ── Polling-Timer (laeuft nur wenn Icons versteckt sind) ──────────────

    unsafe fn start_refresh_timer() {
        if REFRESH_TIMER_PTR.load(Ordering::SeqCst) != 0 {
            return;
        }
        let observer = WALLPAPER_OBSERVER_PTR.load(Ordering::SeqCst);
        if observer == 0 {
            return;
        }
        let timer: *mut Object = msg_send![class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.5f64
            target: observer as *mut Object
            selector: sel!(timerTick)
            userInfo: std::ptr::null_mut::<Object>()
            repeats: objc::runtime::YES
        ];
        if !timer.is_null() {
            let _: () = msg_send![timer, retain];
            REFRESH_TIMER_PTR.store(timer as usize, Ordering::SeqCst);
        }
    }

    unsafe fn stop_refresh_timer() {
        let ptr = REFRESH_TIMER_PTR.swap(0, Ordering::SeqCst);
        if ptr != 0 {
            let timer = ptr as *mut Object;
            let _: () = msg_send![timer, invalidate];
            let _: () = msg_send![timer, release];
        }
        // URL-Cache leeren → beim naechsten Start immer frischer Screenshot
        let url_ptr = LAST_WALLPAPER_URL_PTR.swap(0, Ordering::SeqCst);
        if url_ptr != 0 {
            let _: () = msg_send![url_ptr as *mut Object, release];
        }
    }

    // ── Toggle ─────────────────────────────────────────────────────────────

    pub fn toggle_icon_hiding() {
        if ICONS_HIDDEN.load(Ordering::SeqCst) {
            hide_overlay();
            unsafe {
                stop_refresh_timer();
            }
            ICONS_HIDDEN.store(false, Ordering::SeqCst);
            update_dots_title(false);
            if let Some(h) = APP_HANDLE.get() {
                let _ = h.emit("barlo-icons-state", serde_json::json!({ "hidden": false }));
            }
        } else {
            show_overlay();
            unsafe {
                start_refresh_timer();
            }
            ICONS_HIDDEN.store(true, Ordering::SeqCst);
            update_dots_title(true);
            if let Some(h) = APP_HANDLE.get() {
                let _ = h.emit("barlo-icons-state", serde_json::json!({ "hidden": true }));
            }
        }
    }

    // ── Wallpaper-Observer / Timer-Target ──────────────────────────────────
    // NSWorkspaceDesktopImageDidChangeNotification und NSDistributedNotificationCenter
    // feuern auf macOS 15 Sequoia nicht zuverlaessig → Polling per NSTimer (1s).
    // Der Screenshot des Menüleisten-Streifens (~200x30px) ist minimal teuer.

    pub fn setup_wallpaper_observer() {
        static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
        unsafe {
            if !CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
                if let Some(mut decl) = ClassDecl::new("BarloWallpaperObserver", class!(NSObject)) {
                    extern "C" fn timer_tick(_this: &Object, _cmd: Sel) {
                        if !ICONS_HIDDEN.load(Ordering::SeqCst) {
                            return;
                        }
                        unsafe {
                            show_overlay();
                        }
                    }
                    extern "C" fn restore_overlay(_this: &Object, _cmd: Sel) {
                        if ICONS_HIDDEN.load(Ordering::SeqCst) {
                            unsafe {
                                show_overlay();
                            }
                        }
                    }
                    decl.add_method(sel!(timerTick), timer_tick as extern "C" fn(&Object, Sel));
                    decl.add_method(
                        sel!(restoreOverlay),
                        restore_overlay as extern "C" fn(&Object, Sel),
                    );
                    decl.register();
                }
            }

            let observer: *mut Object = msg_send![class!(BarloWallpaperObserver), new];
            let _: () = msg_send![observer, retain];
            WALLPAPER_OBSERVER_PTR.store(observer as usize, Ordering::SeqCst);
        }
    }

    // ── "⋯" NSStatusItem ───────────────────────────────────────────────────

    pub fn create_dots_status_item() {
        static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
        unsafe {
            if !CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
                if let Some(mut decl) = ClassDecl::new("BarloDotsTarget", class!(NSObject)) {
                    extern "C" fn dots_clicked(_this: &Object, _cmd: Sel, _sender: *mut Object) {
                        eprintln!("[Barlo] dots_clicked!");
                        toggle_icon_hiding();
                    }
                    decl.add_method(
                        sel!(dotsClicked:),
                        dots_clicked as extern "C" fn(&Object, Sel, *mut Object),
                    );
                    decl.register();
                }
            }
            let status_bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
            let item: *mut Object = msg_send![status_bar, statusItemWithLength: -1.0f64];
            if item.is_null() {
                return;
            }
            let button: *mut Object = msg_send![item, button];
            let title: *mut Object = msg_send![class!(NSString),
                stringWithUTF8String: b"\xe2\x8b\xaf\0".as_ptr() as *const i8];
            let _: () = msg_send![button, setTitle: title];
            let handler: *mut Object = msg_send![class!(BarloDotsTarget), new];
            let _: () = msg_send![button, setTarget: handler];
            let _: () = msg_send![button, setAction: sel!(dotsClicked:)];
            let _: () = msg_send![item, retain];
            let _: () = msg_send![handler, retain];
            DOTS_ITEM_PTR.store(item as usize, Ordering::SeqCst);
            DOTS_HANDLER_PTR.store(handler as usize, Ordering::SeqCst);
            eprintln!("[Barlo] Dots-Item erstellt");
        }
    }

    // ── Hidden status item enumeration ────────────────────────────────────

    pub unsafe fn collect_hidden_status_items() -> Vec<StatusItemInfo> {
        let (left_x, right_x, _, _) = match overlay_bounds() {
            Some(b) => b,
            None => return vec![],
        };
        let arr = CGWindowListCopyWindowInfo(1u32, 0u32) as *mut Object;
        if arr.is_null() {
            return vec![];
        }
        let count: usize = msg_send![arr, count];

        let key_layer = nsstring_static("kCGWindowLayer");
        let key_num = nsstring_static("kCGWindowNumber");
        let key_pid = nsstring_static("kCGWindowOwnerPID");
        let key_bounds = nsstring_static("kCGWindowBounds");
        let key_x = nsstring_static("X");
        let key_y = nsstring_static("Y");
        let key_w = nsstring_static("Width");
        let key_h = nsstring_static("Height");

        let our_pid = std::process::id() as i32;
        let mut result = Vec::new();

        for i in 0..count {
            let info: *mut Object = msg_send![arr, objectAtIndex: i];

            // Only level-25 windows (status bar level)
            let layer_obj: *mut Object = msg_send![info, objectForKey: key_layer];
            if layer_obj.is_null() {
                continue;
            }
            let layer: i32 = msg_send![layer_obj, intValue];
            if layer != 25 {
                continue;
            }

            // Skip our own windows
            let pid_obj: *mut Object = msg_send![info, objectForKey: key_pid];
            if !pid_obj.is_null() {
                let pid: i32 = msg_send![pid_obj, intValue];
                if pid == our_pid {
                    continue;
                }
            }

            // Get bounds
            let bounds_dict: *mut Object = msg_send![info, objectForKey: key_bounds];
            if bounds_dict.is_null() {
                continue;
            }

            let x_obj: *mut Object = msg_send![bounds_dict, objectForKey: key_x];
            let y_obj: *mut Object = msg_send![bounds_dict, objectForKey: key_y];
            let w_obj: *mut Object = msg_send![bounds_dict, objectForKey: key_w];
            let h_obj: *mut Object = msg_send![bounds_dict, objectForKey: key_h];
            if x_obj.is_null() || w_obj.is_null() {
                continue;
            }

            let win_x: f64 = msg_send![x_obj, doubleValue];
            let win_y: f64 = if y_obj.is_null() {
                0.0
            } else {
                msg_send![y_obj, doubleValue]
            };
            let win_w: f64 = msg_send![w_obj, doubleValue];
            let win_h: f64 = if h_obj.is_null() {
                22.0
            } else {
                msg_send![h_obj, doubleValue]
            };

            // Filter: center-x must be in the hidden zone
            let center_x = win_x + win_w / 2.0;
            if center_x < left_x || center_x > right_x {
                continue;
            }

            // Skip tiny windows (width < 5)
            if win_w < 5.0 {
                continue;
            }

            // Click position in CG coords (Y=0 at top)
            let click_x = win_x + win_w / 2.0;
            let click_y = win_y + win_h / 2.0;

            // Screenshot of this specific window
            let num_obj: *mut Object = msg_send![info, objectForKey: key_num];
            if num_obj.is_null() {
                continue;
            }
            let wid: u32 = msg_send![num_obj, unsignedIntValue];

            let win_rect = NSRect {
                origin: NSPoint { x: win_x, y: win_y },
                size: NSSize {
                    width: win_w,
                    height: win_h,
                },
            };
            // kCGWindowListOptionIncludingWindow = 8
            let cg_image = CGWindowListCreateImage(win_rect, 8u32, wid, 0u32);
            if cg_image.is_null() {
                continue;
            }

            let logical_size = NSSize {
                width: win_w,
                height: win_h,
            };
            let rep: *mut Object = msg_send![class!(NSBitmapImageRep), alloc];
            let rep: *mut Object = msg_send![rep, initWithCGImage: cg_image];
            let _: () = msg_send![rep, setSize: logical_size];
            CGImageRelease(cg_image);

            let png_data: *mut Object = msg_send![rep,
                representationUsingType: 4u64
                properties: std::ptr::null_mut::<Object>()
            ];
            let _: () = msg_send![rep, release];
            if png_data.is_null() {
                continue;
            }

            let b64: *mut Object = msg_send![png_data, base64EncodedStringWithOptions: 0u64];
            let icon_base64 = ns_str(b64).unwrap_or_default();
            if icon_base64.is_empty() {
                continue;
            }

            result.push(StatusItemInfo {
                icon_base64,
                click_x,
                click_y,
            });
        }

        let _: () = msg_send![arr, release];
        result
    }

    unsafe fn post_click_at_cg(x: f64, y: f64) {
        let pos = NSPoint { x, y };
        // kCGEventLeftMouseDown=1, kCGEventLeftMouseUp=2, kCGHIDEventTap=0
        let down = CGEventCreateMouseEvent(std::ptr::null(), 1u32, pos, 0u32);
        if !down.is_null() {
            CGEventPost(0u32, down);
            CFRelease(down);
        }
        let up = CGEventCreateMouseEvent(std::ptr::null(), 2u32, pos, 0u32);
        if !up.is_null() {
            CGEventPost(0u32, up);
            CFRelease(up);
        }
    }

    /// Overlay kurz ausblenden → Click an Original-Position senden →
    /// nach 1.5s Overlay wieder einblenden (genug Zeit fürs Menü).
    pub fn activate_item(click_x: f64, click_y: f64) {
        if !ICONS_HIDDEN.load(Ordering::SeqCst) {
            return;
        }
        unsafe {
            hide_overlay();
            // Kurz warten damit das Overlay wirklich weg ist, dann klicken
            std::thread::sleep(std::time::Duration::from_millis(80));
            post_click_at_cg(click_x, click_y);
            // Nach 1.5s Overlay wieder zeigen (Menü sollte bis dahin offen/genutzt sein)
            let obs = WALLPAPER_OBSERVER_PTR.load(Ordering::SeqCst);
            if obs != 0 {
                let null = std::ptr::null_mut::<Object>();
                let _: () = msg_send![obs as *mut Object,
                    performSelector: sel!(restoreOverlay)
                    withObject: null
                    afterDelay: 1.5f64];
            }
        }
    }
}

// ── Tauri commands ─────────────────────────────────────────────────────────────

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
fn check_screen_recording() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::check_screen_recording()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[tauri::command]
fn request_screen_recording() {
    #[cfg(target_os = "macos")]
    {
        macos::request_screen_recording()
    }
}

#[tauri::command]
async fn show_settings(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
async fn toggle_barlo_bar(visible: bool, app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("barlo-bar") {
        if visible {
            dock_barlo_bar(&w);
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
}

fn dock_barlo_bar(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let info = macos::get_notch_info();
        // Fenster weit off-screen starten — resize_barlo_bar setzt die finale Position
        let _ = window.set_size(tauri::LogicalSize::new(100.0, 40.0));
        let _ = window.set_position(tauri::LogicalPosition::new(-9999.0, info.menu_bar_height));
    }
}

#[tauri::command]
async fn position_barlo_bar(app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Some(w) = app.get_webview_window("barlo-bar") {
            let info = macos::get_notch_info();
            let _ = w.set_position(tauri::PhysicalPosition::new(
                0i32,
                info.menu_bar_height as i32,
            ));
        }
    }
}

#[tauri::command]
fn get_hidden_status_items() -> Vec<StatusItemInfo> {
    #[cfg(target_os = "macos")]
    {
        unsafe { macos::collect_hidden_status_items() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![]
    }
}

#[tauri::command]
async fn resize_barlo_bar(content_width: f64, app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(w) = app.get_webview_window("barlo-bar") {
        let info = macos::get_notch_info();
        // content_width = tatsächliche Icons-Breite aus DOM
        // + 32px für CSS padding (2×16px)
        let bar_width = (content_width + 32.0).max(32.0);
        let bar_x = info.screen_width - 150.0 - bar_width;
        let _ = w.set_size(tauri::LogicalSize::new(bar_width, 40.0));
        let _ = w.set_position(tauri::LogicalPosition::new(bar_x, info.menu_bar_height));
    }
}

#[tauri::command]
fn activate_status_item(click_x: f64, click_y: f64) {
    #[cfg(target_os = "macos")]
    unsafe {
        macos::activate_item(click_x, click_y);
    }
}

#[tauri::command]
fn toggle_icon_hiding_cmd() {
    #[cfg(target_os = "macos")]
    macos::toggle_icon_hiding();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            APP_HANDLE.set(app.handle().clone()).ok();
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                // Screen Recording Permission beim Start anfordern
                macos::request_screen_recording();
                macos::create_dots_status_item();
                macos::create_anchor_status_item();
                macos::setup_wallpaper_observer();
                // Overlay wird lazy beim ersten Klick erstellt
            }

            if let Some(barlo_bar) = app.get_webview_window("barlo-bar") {
                dock_barlo_bar(&barlo_bar);
                #[cfg(target_os = "macos")]
                unsafe {
                    use objc::runtime::Object;
                    use objc::{msg_send, sel, sel_impl};
                    if let Ok(ptr) = barlo_bar.ns_window() {
                        let ns_win = ptr as *mut Object;
                        let _: () = msg_send![ns_win, setHasShadow: objc::runtime::NO];
                    }
                }
            }

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

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "toggle-barlo-bar" => {
                        if let Some(w) = app.get_webview_window("barlo-bar") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                dock_barlo_bar(&w);
                                let _ = w.show();
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
            check_screen_recording,
            request_screen_recording,
            show_settings,
            toggle_barlo_bar,
            position_barlo_bar,
            get_hidden_status_items,
            resize_barlo_bar,
            activate_status_item,
            toggle_icon_hiding_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
