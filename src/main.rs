#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, FillRect, GdiFlush, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
    UpdateWindow, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS,
    COLOR_WINDOW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, FF_SWISS, FW_BOLD, HBRUSH, HFONT, OUT_TT_PRECIS, TRANSPARENT,
};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, LoadCursorW,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, BST_CHECKED,
    BST_UNCHECKED, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CW_USEDEFAULT,
    GWLP_USERDATA, HMENU, IDC_ARROW, SS_LEFT, SW_HIDE, SW_SHOW, WINDOW_STYLE, WM_CLOSE,
    WM_COMMAND, WM_DESTROY, WM_SETFONT, WM_SETTINGCHANGE, WNDCLASSEXW, WS_CAPTION, WS_CHILD,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const APP_NAME: &str = "NetPulse";
const RUN_REG_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const PERSONALIZE_REG_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

// Control IDs for native Win32 Settings Window
const ID_CHK_ENABLED: usize = 1001;
const ID_CHK_AUTOSTART: usize = 1002;
const ID_CMB_LAYOUT: usize = 1003;
const ID_CMB_FONTSIZE: usize = 1004;
const ID_CMB_COLOR: usize = 1005;
const ID_BTN_RESET: usize = 1006;
const ID_BTN_HIDE: usize = 1007;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrayLayout {
    Stacked = 0,
    SideBySide = 1,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontSizePreset {
    Small = 0,
    Medium = 1,
    Large = 2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorMode {
    Auto = 0,
    White = 1,
    Black = 2,
    Cyan = 3,
    Green = 4,
    Amber = 5,
}

#[derive(Clone)]
struct AppConfig {
    monitor_enabled: bool,
    layout: TrayLayout,
    font_size: FontSizePreset,
    color_mode: ColorMode,
    auto_start: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            monitor_enabled: true,
            layout: TrayLayout::Stacked,
            font_size: FontSizePreset::Medium,
            color_mode: ColorMode::Auto,
            auto_start: is_autostart_enabled(),
        }
    }
}

#[derive(Clone, Default)]
struct NetStats {
    down_bps: u64,
    up_bps: u64,
    session_down_bytes: u64,
    session_up_bytes: u64,
    active_adapter: String,
}

struct SharedState {
    config: AppConfig,
    stats: NetStats,
}

#[derive(Debug)]
enum UserEvent {
    Tick,
    TrayClick,
    Menu(MenuId),
    SettingsChanged,
}

struct SettingsWindow {
    hwnd: HWND,
    lbl_down: HWND,
    lbl_up: HWND,
    lbl_session: HWND,
    lbl_adapter: HWND,
    chk_enabled: HWND,
    chk_autostart: HWND,
    cmb_layout: HWND,
    cmb_fontsize: HWND,
    cmb_color: HWND,
    ui_font: HFONT,
}

struct WindowUserData {
    shared: Arc<Mutex<SharedState>>,
    proxy: EventLoopProxy<UserEvent>,
    chk_enabled: HWND,
    chk_autostart: HWND,
    cmb_layout: HWND,
    cmb_fontsize: HWND,
    cmb_color: HWND,
}

fn main() {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("Failed to create winit event loop");
    let proxy = event_loop.create_proxy();

    // Forward tray & menu events into winit event loop
    let tray_proxy = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = tray_proxy.send_event(UserEvent::TrayClick);
        }
    }));

    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event.id));
    }));

    let shared = Arc::new(Mutex::new(SharedState {
        config: AppConfig::default(),
        stats: NetStats {
            active_adapter: "Detecting...".to_string(),
            ..Default::default()
        },
    }));

    // Build Tray Menu
    let menu = Menu::new();
    let item_open = MenuItem::new("Open Settings", true, None);
    let item_toggle = CheckMenuItem::new("Enable Taskbar Monitor", true, true, None);
    let item_autostart = CheckMenuItem::new(
        "Start with Windows",
        true,
        shared.lock().unwrap().config.auto_start,
        None,
    );
    let item_exit = MenuItem::new("Exit", true, None);

    menu.append(&item_open).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&item_toggle).unwrap();
    menu.append(&item_autostart).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&item_exit).unwrap();

    let initial_icon = render_tray_icon(
        0,
        0,
        true,
        TrayLayout::Stacked,
        FontSizePreset::Medium,
        resolve_text_color(ColorMode::Auto),
    );

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("NetPulse — Initializing...")
        .with_icon(initial_icon)
        .with_menu_on_left_click(false)
        .build()
        .expect("Failed to create tray icon");

    // Spawn 1-second network sampling thread
    let worker_shared = Arc::clone(&shared);
    let worker_proxy = proxy.clone();
    std::thread::spawn(move || {
        let mut prev_sample = sample_network_totals();
        let mut prev_time = Instant::now();

        loop {
            std::thread::sleep(Duration::from_secs(1));

            let enabled = worker_shared.lock().unwrap().config.monitor_enabled;
            if !enabled {
                prev_sample = sample_network_totals();
                prev_time = Instant::now();
                let _ = worker_proxy.send_event(UserEvent::Tick);
                continue;
            }

            let now = Instant::now();
            let elapsed = now.duration_since(prev_time).as_secs_f64().max(0.1);
            let curr = sample_network_totals();

            let delta_down = curr.total_in.saturating_sub(prev_sample.total_in);
            let delta_up = curr.total_out.saturating_sub(prev_sample.total_out);

            let down_bps = (delta_down as f64 / elapsed) as u64;
            let up_bps = (delta_up as f64 / elapsed) as u64;

            {
                let mut state = worker_shared.lock().unwrap();
                state.stats.down_bps = down_bps;
                state.stats.up_bps = up_bps;
                state.stats.session_down_bytes =
                    state.stats.session_down_bytes.saturating_add(delta_down);
                state.stats.session_up_bytes =
                    state.stats.session_up_bytes.saturating_add(delta_up);
                state.stats.active_adapter = curr.adapter_name.clone();
            }

            prev_sample = curr;
            prev_time = now;

            let _ = worker_proxy.send_event(UserEvent::Tick);
        }
    });

    let settings_win = create_settings_window(Arc::clone(&shared), proxy.clone());

    let mut app = NetPulseApp {
        shared,
        tray_icon: Some(tray_icon),
        settings_win,
        id_open: item_open.id().clone(),
        id_toggle: item_toggle.id().clone(),
        id_autostart: item_autostart.id().clone(),
        id_exit: item_exit.id().clone(),
        item_toggle,
        item_autostart,
    };

    event_loop.set_control_flow(ControlFlow::Wait);
    let _ = event_loop.run_app(&mut app);
}

struct NetPulseApp {
    shared: Arc<Mutex<SharedState>>,
    tray_icon: Option<TrayIcon>,
    settings_win: SettingsWindow,
    id_open: MenuId,
    id_toggle: MenuId,
    id_autostart: MenuId,
    id_exit: MenuId,
    item_toggle: CheckMenuItem,
    item_autostart: CheckMenuItem,
}

impl NetPulseApp {
    fn refresh_tray_and_ui(&self) {
        let (cfg, stats) = {
            let state = self.shared.lock().unwrap();
            (state.config.clone(), state.stats.clone())
        };

        let color = resolve_text_color(cfg.color_mode);
        let icon = render_tray_icon(
            stats.down_bps,
            stats.up_bps,
            cfg.monitor_enabled,
            cfg.layout,
            cfg.font_size,
            color,
        );

        if let Some(tray) = &self.tray_icon {
            let _ = tray.set_icon(Some(icon));
            let tip = if cfg.monitor_enabled {
                format!(
                    "NetPulse\n↓ {}/s   ↑ {}/s\nSession: ↓ {} / ↑ {}\n{}",
                    format_speed_long(stats.down_bps),
                    format_speed_long(stats.up_bps),
                    format_bytes(stats.session_down_bytes),
                    format_bytes(stats.session_up_bytes),
                    stats.active_adapter
                )
            } else {
                "NetPulse (Paused)".to_string()
            };
            let _ = tray.set_tooltip(Some(tip));
        }

        self.item_toggle.set_checked(cfg.monitor_enabled);
        self.item_autostart.set_checked(cfg.auto_start);

        unsafe {
            let down_text = to_wide(&format!(
                "Download:  ↓ {}/s",
                format_speed_long(stats.down_bps)
            ));
            let up_text = to_wide(&format!(
                "Upload:    ↑ {}/s",
                format_speed_long(stats.up_bps)
            ));
            let sess_text = to_wide(&format!(
                "Session:   ↓ {}   |   ↑ {}",
                format_bytes(stats.session_down_bytes),
                format_bytes(stats.session_up_bytes)
            ));
            let adp_text = to_wide(&format!("Adapter:   {}", stats.active_adapter));

            let _ = SetWindowTextW(self.settings_win.lbl_down, PCWSTR(down_text.as_ptr()));
            let _ = SetWindowTextW(self.settings_win.lbl_up, PCWSTR(up_text.as_ptr()));
            let _ = SetWindowTextW(self.settings_win.lbl_session, PCWSTR(sess_text.as_ptr()));
            let _ = SetWindowTextW(self.settings_win.lbl_adapter, PCWSTR(adp_text.as_ptr()));

            SendMessageW(
                self.settings_win.chk_enabled,
                BM_SETCHECK,
                WPARAM(if cfg.monitor_enabled {
                    BST_CHECKED.0 as usize
                } else {
                    BST_UNCHECKED.0 as usize
                }),
                LPARAM(0),
            );
            SendMessageW(
                self.settings_win.chk_autostart,
                BM_SETCHECK,
                WPARAM(if cfg.auto_start {
                    BST_CHECKED.0 as usize
                } else {
                    BST_UNCHECKED.0 as usize
                }),
                LPARAM(0),
            );
            SendMessageW(
                self.settings_win.cmb_layout,
                CB_SETCURSEL,
                WPARAM(cfg.layout as usize),
                LPARAM(0),
            );
            SendMessageW(
                self.settings_win.cmb_fontsize,
                CB_SETCURSEL,
                WPARAM(cfg.font_size as usize),
                LPARAM(0),
            );
            SendMessageW(
                self.settings_win.cmb_color,
                CB_SETCURSEL,
                WPARAM(cfg.color_mode as usize),
                LPARAM(0),
            );
        }
    }

    fn show_settings(&self) {
        unsafe {
            let _ = ShowWindow(self.settings_win.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.settings_win.hwnd);
        }
        self.refresh_tray_and_ui();
    }
}

impl ApplicationHandler<UserEvent> for NetPulseApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Tick | UserEvent::SettingsChanged => {
                self.refresh_tray_and_ui();
            }
            UserEvent::TrayClick => {
                self.show_settings();
            }
            UserEvent::Menu(id) => {
                if id == self.id_open {
                    self.show_settings();
                } else if id == self.id_toggle {
                    {
                        let mut state = self.shared.lock().unwrap();
                        state.config.monitor_enabled = !state.config.monitor_enabled;
                    }
                    self.refresh_tray_and_ui();
                } else if id == self.id_autostart {
                    let target = {
                        let mut state = self.shared.lock().unwrap();
                        let next = !state.config.auto_start;
                        if set_autostart(next).is_ok() {
                            state.config.auto_start = next;
                        }
                        state.config.auto_start
                    };
                    self.item_autostart.set_checked(target);
                    self.refresh_tray_and_ui();
                } else if id == self.id_exit {
                    self.tray_icon.take();
                    unsafe {
                        let _ = DeleteObject(self.settings_win.ui_font.into());
                        let _ = DestroyWindow(self.settings_win.hwnd);
                    }
                    event_loop.exit();
                }
            }
        }
    }
}

// ============================================================================
// Network Interface Statistics (Windows IP Helper API: GetIfTable2)
// ============================================================================

struct NetSample {
    total_in: u64,
    total_out: u64,
    adapter_name: String,
}

fn sample_network_totals() -> NetSample {
    let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let mut total_in: u64 = 0;
    let mut total_out: u64 = 0;
    let mut best_adapter = String::from("No active adapter");
    let mut best_traffic: u64 = 0;

    unsafe {
        if GetIfTable2(&mut table_ptr).is_ok() && !table_ptr.is_null() {
            let count = (*table_ptr).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*table_ptr).Table.as_ptr(), count);

            for row in rows {
                // Skip non-operational or loopback interfaces (IF_TYPE_SOFTWARE_LOOPBACK = 24)
                if row.OperStatus != IfOperStatusUp || row.Type == 24 {
                    continue;
                }
                // Skip interfaces without a physical MAC (e.g., virtual/tunnel pseudo-adapters)
                if row.PhysicalAddressLength == 0 {
                    continue;
                }

                let desc = utf16_array_to_string(&row.Description);
                let alias = utf16_array_to_string(&row.Alias);
                let lower = desc.to_lowercase();
                if lower.contains("loopback")
                    || lower.contains("wan miniport")
                    || lower.contains("kernel debug")
                {
                    continue;
                }

                total_in = total_in.saturating_add(row.InOctets);
                total_out = total_out.saturating_add(row.OutOctets);

                let traffic = row.InOctets.saturating_add(row.OutOctets);
                if traffic >= best_traffic {
                    best_traffic = traffic;
                    best_adapter = if alias.is_empty() {
                        desc
                    } else {
                        format!("{alias} ({desc})")
                    };
                }
            }

            FreeMibTable(table_ptr as *const c_void);
        }
    }

    NetSample {
        total_in,
        total_out,
        adapter_name: best_adapter,
    }
}

fn utf16_array_to_string(slice: &[u16]) -> String {
    let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..len]).trim().to_string()
}

// ============================================================================
// Theme Detection & Formatting Helpers
// ============================================================================

fn is_system_light_taskbar() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(PERSONALIZE_REG_PATH, KEY_READ) {
        if let Ok(val) = key.get_value::<u32, _>("SystemUsesLightTheme") {
            return val == 1;
        }
    }
    false
}

fn resolve_text_color(mode: ColorMode) -> (u8, u8, u8) {
    match mode {
        ColorMode::Auto => {
            if is_system_light_taskbar() {
                (15, 15, 15) // Crisp dark text for Light Taskbar
            } else {
                (255, 255, 255) // Crisp white text for Dark Taskbar
            }
        }
        ColorMode::White => (255, 255, 255),
        ColorMode::Black => (15, 15, 15),
        ColorMode::Cyan => (0, 229, 255),
        ColorMode::Green => (74, 222, 128),
        ColorMode::Amber => (251, 191, 36),
    }
}

fn format_speed_compact(bps: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bps as f64;
    if b < KB {
        format!("{bps}B")
    } else if b < MB {
        let v = b / KB;
        if v >= 100.0 {
            format!("{:.0}K", v)
        } else {
            format!("{:.1}K", v)
        }
    } else if b < GB {
        let v = b / MB;
        if v >= 100.0 {
            format!("{:.0}M", v)
        } else {
            format!("{:.1}M", v)
        }
    } else {
        format!("{:.1}G", b / GB)
    }
}

fn format_speed_long(bps: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bps as f64;
    if b < KB {
        format!("{bps} B")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else if b < GB {
        format!("{:.2} MB", b / MB)
    } else {
        format!("{:.2} GB", b / GB)
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else if b < GB {
        format!("{:.2} MB", b / MB)
    } else {
        format!("{:.2} GB", b / GB)
    }
}

// ============================================================================
// Dynamic GDI Tray Icon Renderer (True Alpha Antialiased Text)
// ============================================================================

fn render_tray_icon(
    down_bps: u64,
    up_bps: u64,
    enabled: bool,
    layout: TrayLayout,
    font_size: FontSizePreset,
    rgb: (u8, u8, u8),
) -> Icon {
    const SIZE: i32 = 48; // Crisp 48x48 physical pixels (scales cleanly at 100%-300% DPI)

    let (line1, line2) = if !enabled {
        ("NET".to_string(), "OFF".to_string())
    } else {
        (
            format!("↓{}", format_speed_compact(down_bps)),
            format!("↑{}", format_speed_compact(up_bps)),
        )
    };

    let font_px = match (layout, font_size) {
        (TrayLayout::Stacked, FontSizePreset::Small) => 17,
        (TrayLayout::Stacked, FontSizePreset::Medium) => 20,
        (TrayLayout::Stacked, FontSizePreset::Large) => 23,
        (TrayLayout::SideBySide, FontSizePreset::Small) => 12,
        (TrayLayout::SideBySide, FontSizePreset::Medium) => 14,
        (TrayLayout::SideBySide, FontSizePreset::Large) => 16,
    };

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    unsafe {
        let hdc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE, // Top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        if let Ok(hbmp) = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
        {
            let old_bmp = SelectObject(hdc, hbmp.into());

            // Clear to pure black (0x00000000)
            let bg_brush = CreateSolidBrush(COLORREF(0));
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: SIZE,
                bottom: SIZE,
            };
            FillRect(hdc, &full_rect, bg_brush);
            let _ = DeleteObject(bg_brush.into());

            let font = CreateFontW(
                font_px,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_TT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
                w!("Segoe UI"),
            );
            let old_font = SelectObject(hdc, font.into());

            // Draw white text on black background to extract antialiased alpha coverage
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FF_FFFF));

            match layout {
                TrayLayout::Stacked => {
                    let mut r1 = RECT {
                        left: 0,
                        top: 1,
                        right: SIZE,
                        bottom: SIZE / 2 + 2,
                    };
                    let mut r2 = RECT {
                        left: 0,
                        top: SIZE / 2 - 2,
                        right: SIZE,
                        bottom: SIZE - 1,
                    };
                    let mut w1 = to_wide(&line1);
                    let mut w2 = to_wide(&line2);
                    let len1 = w1.len() - 1;
                    let len2 = w2.len() - 1;
                    DrawTextW(
                        hdc,
                        &mut w1[..len1],
                        &mut r1,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                    DrawTextW(
                        hdc,
                        &mut w2[..len2],
                        &mut r2,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                }
                TrayLayout::SideBySide => {
                    let mut r_full = RECT {
                        left: 0,
                        top: 0,
                        right: SIZE,
                        bottom: SIZE,
                    };
                    let combined = format!("{line1} {line2}");
                    let mut w_all = to_wide(&combined);
                    let len_all = w_all.len() - 1;
                    DrawTextW(
                        hdc,
                        &mut w_all[..len_all],
                        &mut r_full,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                }
            }

            let _ = GdiFlush();

            if !bits_ptr.is_null() {
                let src =
                    std::slice::from_raw_parts(bits_ptr as *const u8, (SIZE * SIZE * 4) as usize);
                for (dst_px, src_px) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    let coverage = src_px[0].max(src_px[1]).max(src_px[2]);
                    dst_px[0] = rgb.0;
                    dst_px[1] = rgb.1;
                    dst_px[2] = rgb.2;
                    dst_px[3] = coverage;
                }
            }

            SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
            SelectObject(hdc, old_bmp);
            let _ = DeleteObject(hbmp.into());
        }
        let _ = DeleteDC(hdc);
    }

    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("Valid RGBA icon")
}

// ============================================================================
// Windows Startup Registry (HKCU\Software\Microsoft\Windows\CurrentVersion\Run)
// ============================================================================

fn is_autostart_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(RUN_REG_PATH, KEY_READ) {
        return key.get_value::<String, _>(APP_NAME).is_ok();
    }
    false
}

fn set_autostart(enable: bool) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(RUN_REG_PATH, KEY_WRITE)?;
    if enable {
        let exe_path = std::env::current_exe()?;
        let quoted = format!("\"{}\"", exe_path.display());
        key.set_value(APP_NAME, &quoted)?;
    } else {
        let _ = key.delete_value(APP_NAME);
    }
    Ok(())
}

// ============================================================================
// Native Win32 Settings & Control Window
// ============================================================================

fn create_settings_window(
    shared: Arc<Mutex<SharedState>>,
    proxy: EventLoopProxy<UserEvent>,
) -> SettingsWindow {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("NetPulseSettingsClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(settings_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut c_void),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("NetPulse — Taskbar Speed Monitor"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            400,
            390,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .expect("Failed to create settings window");

        // Enable Windows 11 Dark Title Bar if system is in Dark Mode
        let dark: u32 = if is_system_light_taskbar() { 0 } else { 1 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );

        let ui_font = CreateFontW(
            -14,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
            w!("Segoe UI"),
        );

        let make_ctrl = |cls: PCWSTR,
                         text: PCWSTR,
                         style: u32,
                         x: i32,
                         y: i32,
                         w: i32,
                         h: i32,
                         id: usize|
         -> HWND {
            let child = CreateWindowExW(
                Default::default(),
                cls,
                text,
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
                x,
                y,
                w,
                h,
                Some(hwnd),
                Some(HMENU(id as *mut c_void)),
                Some(hinstance.into()),
                None,
            )
            .unwrap();
            SendMessageW(
                child,
                WM_SETFONT,
                WPARAM(ui_font.0 as usize),
                LPARAM(1),
            );
            child
        };

        let lbl_down = make_ctrl(
            w!("STATIC"),
            w!("Download:  ↓ 0 B/s"),
            SS_LEFT.0,
            20,
            18,
            345,
            22,
            0,
        );
        let lbl_up = make_ctrl(
            w!("STATIC"),
            w!("Upload:    ↑ 0 B/s"),
            SS_LEFT.0,
            20,
            44,
            345,
            22,
            0,
        );
        let lbl_session = make_ctrl(
            w!("STATIC"),
            w!("Session:   ↓ 0 B   |   ↑ 0 B"),
            SS_LEFT.0,
            20,
            72,
            345,
            22,
            0,
        );
        let lbl_adapter = make_ctrl(
            w!("STATIC"),
            w!("Adapter:   Detecting..."),
            SS_LEFT.0,
            20,
            98,
            345,
            22,
            0,
        );

        let chk_enabled = make_ctrl(
            w!("BUTTON"),
            w!("Enable Taskbar Speed Monitor"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            20,
            136,
            300,
            24,
            ID_CHK_ENABLED,
        );

        let _ = make_ctrl(
            w!("STATIC"),
            w!("Taskbar Layout:"),
            SS_LEFT.0,
            20,
            174,
            120,
            22,
            0,
        );
        let cmb_layout = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            150,
            170,
            215,
            120,
            ID_CMB_LAYOUT,
        );
        for opt in [w!("Stacked (↓ / ↑)"), w!("Side-by-Side (↓ ↑)")] {
            SendMessageW(
                cmb_layout,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(opt.as_ptr() as isize),
            );
        }

        let _ = make_ctrl(
            w!("STATIC"),
            w!("Font Size:"),
            SS_LEFT.0,
            20,
            208,
            120,
            22,
            0,
        );
        let cmb_fontsize = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            150,
            204,
            215,
            120,
            ID_CMB_FONTSIZE,
        );
        for opt in [w!("Small"), w!("Medium"), w!("Large")] {
            SendMessageW(
                cmb_fontsize,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(opt.as_ptr() as isize),
            );
        }

        let _ = make_ctrl(
            w!("STATIC"),
            w!("Text Color:"),
            SS_LEFT.0,
            20,
            242,
            120,
            22,
            0,
        );
        let cmb_color = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            150,
            238,
            215,
            160,
            ID_CMB_COLOR,
        );
        for opt in [
            w!("Auto (System Theme)"),
            w!("White"),
            w!("Black"),
            w!("Cyan"),
            w!("Green"),
            w!("Amber"),
        ] {
            SendMessageW(
                cmb_color,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(opt.as_ptr() as isize),
            );
        }

        let chk_autostart = make_ctrl(
            w!("BUTTON"),
            w!("Start automatically with Windows"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            20,
            278,
            300,
            24,
            ID_CHK_AUTOSTART,
        );

        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Reset Session"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            20,
            314,
            130,
            30,
            ID_BTN_RESET,
        );
        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Hide to Tray"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            235,
            314,
            130,
            30,
            ID_BTN_HIDE,
        );

        let user_data = Box::new(WindowUserData {
            shared,
            proxy,
            chk_enabled,
            chk_autostart,
            cmb_layout,
            cmb_fontsize,
            cmb_color,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(user_data) as isize);
        let _ = UpdateWindow(hwnd);

        SettingsWindow {
            hwnd,
            lbl_down,
            lbl_up,
            lbl_session,
            lbl_adapter,
            chk_enabled,
            chk_autostart,
            cmb_layout,
            cmb_fontsize,
            cmb_color,
            ui_font,
        }
    }
}

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLOSE => {
            // Minimize to background tray instead of exiting
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowUserData;
            if !ptr.is_null() {
                let data = &*ptr;
                let _ = data.proxy.send_event(UserEvent::SettingsChanged);
            }
            let _ = InvalidateRect(Some(hwnd), None, true);
            LRESULT(0)
        }
        WM_COMMAND => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowUserData;
            if !ptr.is_null() {
                let data = &*ptr;
                let ctrl_id = wparam.0 & 0xFFFF;
                let notif = (wparam.0 >> 16) & 0xFFFF;

                match ctrl_id {
                    ID_CHK_ENABLED => {
                        let checked =
                            SendMessageW(data.chk_enabled, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
                                as u32
                                == BST_CHECKED.0;
                        data.shared.lock().unwrap().config.monitor_enabled = checked;
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CHK_AUTOSTART => {
                        let checked =
                            SendMessageW(data.chk_autostart, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
                                as u32
                                == BST_CHECKED.0;
                        if set_autostart(checked).is_ok() {
                            data.shared.lock().unwrap().config.auto_start = checked;
                        }
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_LAYOUT if notif == 1 => {
                        let idx =
                            SendMessageW(data.cmb_layout, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        data.shared.lock().unwrap().config.layout = if idx == 1 {
                            TrayLayout::SideBySide
                        } else {
                            TrayLayout::Stacked
                        };
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_FONTSIZE if notif == 1 => {
                        let idx =
                            SendMessageW(data.cmb_fontsize, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        data.shared.lock().unwrap().config.font_size = match idx {
                            0 => FontSizePreset::Small,
                            2 => FontSizePreset::Large,
                            _ => FontSizePreset::Medium,
                        };
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_COLOR if notif == 1 => {
                        let idx =
                            SendMessageW(data.cmb_color, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        data.shared.lock().unwrap().config.color_mode = match idx {
                            1 => ColorMode::White,
                            2 => ColorMode::Black,
                            3 => ColorMode::Cyan,
                            4 => ColorMode::Green,
                            5 => ColorMode::Amber,
                            _ => ColorMode::Auto,
                        };
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_BTN_RESET => {
                        let mut state = data.shared.lock().unwrap();
                        state.stats.session_down_bytes = 0;
                        state.stats.session_up_bytes = 0;
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_BTN_HIDE => {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowUserData;
            if !ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
