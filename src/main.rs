#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, FillRect, GdiFlush, GetDC, InvalidateRect, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, UpdateWindow, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
    FF_SWISS, FW_BOLD, FW_EXTRABOLD, HBRUSH, HDC, HFONT, OUT_TT_PRECIS, TRANSPARENT,
};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::{
    ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextW, LoadCursorW, RegisterClassExW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    UpdateLayeredWindow, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CW_USEDEFAULT, GWLP_USERDATA, HMENU, HWND_TOPMOST,
    IDC_ARROW, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, ULW_ALPHA, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_SETFONT, WM_SETTINGCHANGE, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
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

// Standard Win32 constants
const BST_CHECKED_VAL: usize = 1;
const BST_UNCHECKED_VAL: usize = 0;
const SS_LEFT_STYLE: u32 = 0;
const ES_AUTOHSCROLL_STYLE: u32 = 0x0080;
const LBS_NOTIFY_STYLE: u32 = 0x0001;
const CBN_SELCHANGE_NOTIF: usize = 1;
const LBN_SELCHANGE_NOTIF: usize = 1;

// Overlay widget dimensions (positioned next to Windows 11 clock)
const OVERLAY_W: i32 = 165;
const OVERLAY_H: i32 = 44;

// Control IDs for native Win32 Settings Window
const ID_CHK_ENABLED: usize = 1001;
const ID_CHK_AUTOSTART: usize = 1002;
const ID_CMB_LAYOUT: usize = 1003;
const ID_CMB_FONTSIZE: usize = 1004;
const ID_CMB_COLOR: usize = 1005;
const ID_BTN_RESET: usize = 1006;
const ID_BTN_HIDE: usize = 1007;
const ID_CHK_OVERLAY: usize = 1008;

// Dynamic Speed Color Rule Control IDs
const ID_CHK_DYN_ENABLED: usize = 1010;
const ID_CMB_DYN_METRIC: usize = 1011;
const ID_LST_RULES: usize = 1012;
const ID_EDT_MIN: usize = 1013;
const ID_CMB_MIN_UNIT: usize = 1014;
const ID_EDT_MAX: usize = 1015;
const ID_CMB_MAX_UNIT: usize = 1016;
const ID_CMB_RULE_COLOR: usize = 1017;
const ID_EDT_RULE_HEX: usize = 1018;
const ID_BTN_PICK_COLOR: usize = 1019;
const ID_BTN_ADD_RULE: usize = 1020;
const ID_BTN_UPD_RULE: usize = 1021;
const ID_BTN_DEL_RULE: usize = 1022;

// Data Usage Limit Control IDs
const ID_CHK_LIMIT_ENABLED: usize = 1030;
const ID_EDT_LIMIT_VAL: usize = 1031;
const ID_CMB_LIMIT_UNIT: usize = 1032;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrayLayout {
    Stacked = 0,
    SideBySide = 1,
    ExtraLargeNumbersOnly = 2,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedMetric {
    Download = 0,
    Total = 1,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeedUnit {
    Unlimited = 0,
    KBps = 1,
    MBps = 2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DataUnit {
    MB = 0,
    GB = 1,
}

#[derive(Clone)]
struct ColorRule {
    min_val: f64,
    min_unit: SpeedUnit, // KBps or MBps
    max_val: f64,
    max_unit: SpeedUnit, // Unlimited, KBps, or MBps
    is_auto: bool,
    rgb: (u8, u8, u8),
    color_label: String,
}

impl ColorRule {
    fn min_bps(&self) -> u64 {
        let mul = match self.min_unit {
            SpeedUnit::KBps => 1024.0,
            SpeedUnit::MBps => 1024.0 * 1024.0,
            SpeedUnit::Unlimited => 0.0,
        };
        (self.min_val.max(0.0) * mul) as u64
    }

    fn max_bps(&self) -> Option<u64> {
        match self.max_unit {
            SpeedUnit::Unlimited => None,
            SpeedUnit::KBps => Some((self.max_val.max(0.0) * 1024.0) as u64),
            SpeedUnit::MBps => Some((self.max_val.max(0.0) * 1024.0 * 1024.0) as u64),
        }
    }

    fn matches(&self, bps: u64) -> bool {
        if bps < self.min_bps() {
            return false;
        }
        match self.max_bps() {
            Some(max) => bps < max,
            None => true,
        }
    }

    fn display_text(&self) -> String {
        let min_u = if self.min_unit == SpeedUnit::MBps {
            "MB/s"
        } else {
            "KB/s"
        };
        let max_str = match self.max_unit {
            SpeedUnit::Unlimited => "Unlimited".to_string(),
            SpeedUnit::KBps => format!("{} KB/s", trim_num(self.max_val)),
            SpeedUnit::MBps => format!("{} MB/s", trim_num(self.max_val)),
        };
        format!(
            "{} {} - {}  ->  {} (#{:02X}{:02X}{:02X})",
            trim_num(self.min_val),
            min_u,
            max_str,
            self.color_label,
            self.rgb.0,
            self.rgb.1,
            self.rgb.2
        )
    }
}

#[derive(Clone)]
struct AppConfig {
    monitor_enabled: bool,
    layout: TrayLayout,
    font_size: FontSizePreset,
    color_mode: ColorMode,
    auto_start: bool,
    show_overlay: bool,
    dynamic_enabled: bool,
    dynamic_metric: SpeedMetric,
    rules: Vec<ColorRule>,
    limit_enabled: bool,
    limit_value: f64,
    limit_unit: DataUnit,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            monitor_enabled: true,
            layout: TrayLayout::Stacked,
            font_size: FontSizePreset::Medium,
            color_mode: ColorMode::Auto,
            auto_start: is_autostart_enabled(),
            show_overlay: false,
            dynamic_enabled: true,
            dynamic_metric: SpeedMetric::Download,
            rules: default_rules(),
            limit_enabled: false,
            limit_value: 1.0,
            limit_unit: DataUnit::GB,
        }
    }
}

impl AppConfig {
    fn limit_bytes(&self) -> u64 {
        let mul = match self.limit_unit {
            DataUnit::MB => 1024.0 * 1024.0,
            DataUnit::GB => 1024.0 * 1024.0 * 1024.0,
        };
        (self.limit_value.max(0.1) * mul) as u64
    }
}

fn default_rules() -> Vec<ColorRule> {
    vec![
        ColorRule {
            min_val: 0.0,
            min_unit: SpeedUnit::KBps,
            max_val: 500.0,
            max_unit: SpeedUnit::KBps,
            is_auto: true,
            rgb: (255, 255, 255),
            color_label: "Auto".to_string(),
        },
        ColorRule {
            min_val: 500.0,
            min_unit: SpeedUnit::KBps,
            max_val: 2.0,
            max_unit: SpeedUnit::MBps,
            is_auto: false,
            rgb: (74, 222, 128),
            color_label: "Green".to_string(),
        },
        ColorRule {
            min_val: 2.0,
            min_unit: SpeedUnit::MBps,
            max_val: 10.0,
            max_unit: SpeedUnit::MBps,
            is_auto: false,
            rgb: (251, 191, 36),
            color_label: "Amber".to_string(),
        },
        ColorRule {
            min_val: 10.0,
            min_unit: SpeedUnit::MBps,
            max_val: 0.0,
            max_unit: SpeedUnit::Unlimited,
            is_auto: false,
            rgb: (239, 68, 68),
            color_label: "Red".to_string(),
        },
    ]
}

#[derive(Clone, Default)]
struct NetStats {
    down_bps: u64,
    up_bps: u64,
    session_down_bytes: u64,
    session_up_bytes: u64,
    active_adapter: String,
    alert_triggered: bool,
    tick_phase: bool,
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
    chk_overlay: HWND,
    cmb_layout: HWND,
    cmb_fontsize: HWND,
    cmb_color: HWND,
    chk_dyn_enabled: HWND,
    cmb_dyn_metric: HWND,
    lst_rules: HWND,
    chk_limit_enabled: HWND,
    edt_limit_val: HWND,
    cmb_limit_unit: HWND,
    ui_font: HFONT,
}

struct WindowUserData {
    shared: Arc<Mutex<SharedState>>,
    proxy: EventLoopProxy<UserEvent>,
    chk_enabled: HWND,
    chk_autostart: HWND,
    chk_overlay: HWND,
    cmb_layout: HWND,
    cmb_fontsize: HWND,
    cmb_color: HWND,
    chk_dyn_enabled: HWND,
    cmb_dyn_metric: HWND,
    lst_rules: HWND,
    edt_min: HWND,
    cmb_min_unit: HWND,
    edt_max: HWND,
    cmb_max_unit: HWND,
    cmb_rule_color: HWND,
    edt_rule_hex: HWND,
    chk_limit_enabled: HWND,
    edt_limit_val: HWND,
    cmb_limit_unit: HWND,
    overlay_hwnd: HWND,
}

fn main() {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("Failed to create winit event loop");
    let proxy = event_loop.create_proxy();

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

    let loaded_config = load_config();
    let init_autostart = loaded_config.auto_start;
    let init_overlay = loaded_config.show_overlay;

    let shared = Arc::new(Mutex::new(SharedState {
        config: loaded_config,
        stats: NetStats {
            active_adapter: "Detecting...".to_string(),
            ..Default::default()
        },
    }));

    let menu = Menu::new();
    let item_open = MenuItem::new("Open Settings", true, None);
    let item_toggle = CheckMenuItem::new("Enable Taskbar Monitor", true, true, None);
    let item_overlay = CheckMenuItem::new("Show Large Taskbar Overlay", true, init_overlay, None);
    let item_autostart = CheckMenuItem::new("Start with Windows", true, init_autostart, None);
    let item_exit = MenuItem::new("Exit", true, None);

    menu.append(&item_open).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&item_toggle).unwrap();
    menu.append(&item_overlay).unwrap();
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
                state.stats.tick_phase = !state.stats.tick_phase;
            }

            prev_sample = curr;
            prev_time = now;

            let _ = worker_proxy.send_event(UserEvent::Tick);
        }
    });

    let overlay_hwnd = create_overlay_window();
    let settings_win = create_settings_window(Arc::clone(&shared), proxy.clone(), overlay_hwnd);

    let mut app = NetPulseApp {
        shared,
        tray_icon: Some(tray_icon),
        settings_win,
        overlay_hwnd,
        id_open: item_open.id().clone(),
        id_toggle: item_toggle.id().clone(),
        id_overlay: item_overlay.id().clone(),
        id_autostart: item_autostart.id().clone(),
        id_exit: item_exit.id().clone(),
        item_toggle,
        item_overlay,
        item_autostart,
    };

    event_loop.set_control_flow(ControlFlow::Wait);
    let _ = event_loop.run_app(&mut app);
}

struct NetPulseApp {
    shared: Arc<Mutex<SharedState>>,
    tray_icon: Option<TrayIcon>,
    settings_win: SettingsWindow,
    overlay_hwnd: HWND,
    id_open: MenuId,
    id_toggle: MenuId,
    id_overlay: MenuId,
    id_autostart: MenuId,
    id_exit: MenuId,
    item_toggle: CheckMenuItem,
    item_overlay: CheckMenuItem,
    item_autostart: CheckMenuItem,
}

impl NetPulseApp {
    fn refresh_tray_and_ui(&self, sync_controls: bool) {
        let (cfg, stats, fire_alert) = {
            let mut state = self.shared.lock().unwrap();
            let total_session = state
                .stats
                .session_down_bytes
                .saturating_add(state.stats.session_up_bytes);
            let limit_exceeded =
                state.config.limit_enabled && total_session >= state.config.limit_bytes();

            let mut fire = false;
            if limit_exceeded && !state.stats.alert_triggered {
                state.stats.alert_triggered = true;
                fire = true;
            }
            (state.config.clone(), state.stats.clone(), fire)
        };

        let total_session = stats
            .session_down_bytes
            .saturating_add(stats.session_up_bytes);
        let limit_exceeded = cfg.limit_enabled && total_session >= cfg.limit_bytes();

        if fire_alert {
            show_limit_balloon(
                self.overlay_hwnd,
                total_session,
                cfg.limit_bytes(),
            );
        }

        let color = if limit_exceeded {
            if stats.tick_phase {
                (239, 68, 68) // Alert Red
            } else {
                (255, 140, 140) // Flashing Alert
            }
        } else {
            resolve_effective_color(&cfg, stats.down_bps, stats.up_bps)
        };

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
                let alert_tag = if limit_exceeded { " [LIMIT EXCEEDED!]" } else { "" };
                format!(
                    "NetPulse{alert_tag}\n↓ {}/s   ↑ {}/s\nSession: ↓ {} / ↑ {}\n{}",
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

        update_overlay_window(
            self.overlay_hwnd,
            cfg.show_overlay,
            cfg.monitor_enabled,
            stats.down_bps,
            stats.up_bps,
            color,
            limit_exceeded,
        );

        self.item_toggle.set_checked(cfg.monitor_enabled);
        self.item_overlay.set_checked(cfg.show_overlay);
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
                "Session:   ↓ {}   |   ↑ {}{}",
                format_bytes(stats.session_down_bytes),
                format_bytes(stats.session_up_bytes),
                if limit_exceeded { "  [LIMIT!]" } else { "" }
            ));
            let adp_text = to_wide(&format!("Adapter:   {}", stats.active_adapter));

            let _ = SetWindowTextW(self.settings_win.lbl_down, PCWSTR(down_text.as_ptr()));
            let _ = SetWindowTextW(self.settings_win.lbl_up, PCWSTR(up_text.as_ptr()));
            let _ = SetWindowTextW(self.settings_win.lbl_session, PCWSTR(sess_text.as_ptr()));
            let _ = SetWindowTextW(self.settings_win.lbl_adapter, PCWSTR(adp_text.as_ptr()));

            if sync_controls {
                SendMessageW(
                    self.settings_win.chk_enabled,
                    BM_SETCHECK,
                    WPARAM(if cfg.monitor_enabled {
                        BST_CHECKED_VAL
                    } else {
                        BST_UNCHECKED_VAL
                    }),
                    LPARAM(0),
                );
                SendMessageW(
                    self.settings_win.chk_overlay,
                    BM_SETCHECK,
                    WPARAM(if cfg.show_overlay {
                        BST_CHECKED_VAL
                    } else {
                        BST_UNCHECKED_VAL
                    }),
                    LPARAM(0),
                );
                SendMessageW(
                    self.settings_win.chk_autostart,
                    BM_SETCHECK,
                    WPARAM(if cfg.auto_start {
                        BST_CHECKED_VAL
                    } else {
                        BST_UNCHECKED_VAL
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
                SendMessageW(
                    self.settings_win.chk_dyn_enabled,
                    BM_SETCHECK,
                    WPARAM(if cfg.dynamic_enabled {
                        BST_CHECKED_VAL
                    } else {
                        BST_UNCHECKED_VAL
                    }),
                    LPARAM(0),
                );
                SendMessageW(
                    self.settings_win.cmb_dyn_metric,
                    CB_SETCURSEL,
                    WPARAM(cfg.dynamic_metric as usize),
                    LPARAM(0),
                );
                SendMessageW(
                    self.settings_win.chk_limit_enabled,
                    BM_SETCHECK,
                    WPARAM(if cfg.limit_enabled {
                        BST_CHECKED_VAL
                    } else {
                        BST_UNCHECKED_VAL
                    }),
                    LPARAM(0),
                );
                let lim_w = to_wide(&trim_num(cfg.limit_value));
                let _ = SetWindowTextW(self.settings_win.edt_limit_val, PCWSTR(lim_w.as_ptr()));
                SendMessageW(
                    self.settings_win.cmb_limit_unit,
                    CB_SETCURSEL,
                    WPARAM(cfg.limit_unit as usize),
                    LPARAM(0),
                );
                populate_rules_listbox(self.settings_win.lst_rules, &cfg.rules);
            }
        }
    }

    fn show_settings(&self) {
        unsafe {
            let _ = ShowWindow(self.settings_win.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.settings_win.hwnd);
        }
        self.refresh_tray_and_ui(true);
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
            UserEvent::Tick => {
                self.refresh_tray_and_ui(false);
            }
            UserEvent::SettingsChanged => {
                let cfg = self.shared.lock().unwrap().config.clone();
                save_config(&cfg);
                self.refresh_tray_and_ui(true);
            }
            UserEvent::TrayClick => {
                self.show_settings();
            }
            UserEvent::Menu(id) => {
                if id == self.id_open {
                    self.show_settings();
                } else if id == self.id_toggle {
                    let cfg = {
                        let mut state = self.shared.lock().unwrap();
                        state.config.monitor_enabled = !state.config.monitor_enabled;
                        state.config.clone()
                    };
                    save_config(&cfg);
                    self.refresh_tray_and_ui(true);
                } else if id == self.id_overlay {
                    let cfg = {
                        let mut state = self.shared.lock().unwrap();
                        state.config.show_overlay = !state.config.show_overlay;
                        state.config.clone()
                    };
                    save_config(&cfg);
                    self.refresh_tray_and_ui(true);
                } else if id == self.id_autostart {
                    let (target, cfg) = {
                        let mut state = self.shared.lock().unwrap();
                        let next = !state.config.auto_start;
                        if set_autostart(next).is_ok() {
                            state.config.auto_start = next;
                        }
                        (state.config.auto_start, state.config.clone())
                    };
                    save_config(&cfg);
                    self.item_autostart.set_checked(target);
                    self.refresh_tray_and_ui(true);
                } else if id == self.id_exit {
                    delete_limit_balloon(self.overlay_hwnd);
                    self.tray_icon.take();
                    unsafe {
                        let _ = DeleteObject(self.settings_win.ui_font);
                        let _ = DestroyWindow(self.overlay_hwnd);
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
                if row.OperStatus != IfOperStatusUp || row.Type == 24 {
                    continue;
                }
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
// Color Resolution & Dynamic Thresholds
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
                (15, 15, 15)
            } else {
                (255, 255, 255)
            }
        }
        ColorMode::White => (255, 255, 255),
        ColorMode::Black => (15, 15, 15),
        ColorMode::Cyan => (0, 229, 255),
        ColorMode::Green => (74, 222, 128),
        ColorMode::Amber => (251, 191, 36),
    }
}

fn resolve_effective_color(cfg: &AppConfig, down_bps: u64, up_bps: u64) -> (u8, u8, u8) {
    if cfg.dynamic_enabled {
        let speed = match cfg.dynamic_metric {
            SpeedMetric::Download => down_bps,
            SpeedMetric::Total => down_bps.saturating_add(up_bps),
        };
        for rule in &cfg.rules {
            if rule.matches(speed) {
                if rule.is_auto {
                    return resolve_text_color(ColorMode::Auto);
                }
                return rule.rgb;
            }
        }
    }
    resolve_text_color(cfg.color_mode)
}

fn preset_color_options() -> [(&'static str, bool, (u8, u8, u8)); 12] {
    [
        ("Auto (Theme)", true, (255, 255, 255)),
        ("White", false, (255, 255, 255)),
        ("Black", false, (15, 15, 15)),
        ("Cyan", false, (0, 229, 255)),
        ("Green", false, (74, 222, 128)),
        ("Lime", false, (163, 230, 53)),
        ("Yellow", false, (250, 204, 21)),
        ("Amber", false, (251, 191, 36)),
        ("Orange", false, (251, 146, 60)),
        ("Red", false, (239, 68, 68)),
        ("Magenta", false, (232, 121, 249)),
        ("Custom Hex", false, (255, 255, 255)),
    ]
}

fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

fn trim_num(v: f64) -> String {
    if (v - v.round()).abs() < 0.001 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.1}", v)
    }
}

// ============================================================================
// Speed & Byte Formatting Helpers
// ============================================================================

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

fn format_speed_numbers_only(bps: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bps as f64;
    if b < KB {
        format!("{bps}")
    } else if b < MB {
        format!("{:.0}K", b / KB)
    } else if b < GB {
        let v = b / MB;
        if v >= 10.0 {
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
// Dynamic 32x32 GDI Tray Icon Renderer
// ============================================================================

fn render_tray_icon(
    down_bps: u64,
    up_bps: u64,
    enabled: bool,
    layout: TrayLayout,
    font_size: FontSizePreset,
    rgb: (u8, u8, u8),
) -> Icon {
    const SIZE: i32 = 32;

    let (line1, line2) = if !enabled {
        ("NET".to_string(), "OFF".to_string())
    } else {
        match layout {
            TrayLayout::ExtraLargeNumbersOnly => (
                format_speed_numbers_only(down_bps),
                format_speed_numbers_only(up_bps),
            ),
            _ => (
                format!("↓{}", format_speed_compact(down_bps)),
                format!("↑{}", format_speed_compact(up_bps)),
            ),
        }
    };

    let font_px = match (layout, font_size) {
        (TrayLayout::ExtraLargeNumbersOnly, FontSizePreset::Small) => 17,
        (TrayLayout::ExtraLargeNumbersOnly, FontSizePreset::Medium) => 19,
        (TrayLayout::ExtraLargeNumbersOnly, FontSizePreset::Large) => 22,
        (TrayLayout::Stacked, FontSizePreset::Small) => 13,
        (TrayLayout::Stacked, FontSizePreset::Medium) => 15,
        (TrayLayout::Stacked, FontSizePreset::Large) => 17,
        (TrayLayout::SideBySide, FontSizePreset::Small) => 9,
        (TrayLayout::SideBySide, FontSizePreset::Medium) => 11,
        (TrayLayout::SideBySide, FontSizePreset::Large) => 12,
    };

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    unsafe {
        let hdc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        if let Ok(hbmp) = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0) {
            let old_bmp = SelectObject(hdc, hbmp);

            let bg_brush = CreateSolidBrush(COLORREF(0));
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: SIZE,
                bottom: SIZE,
            };
            FillRect(hdc, &full_rect, bg_brush);
            let _ = DeleteObject(bg_brush);

            let font = CreateFontW(
                font_px,
                0,
                0,
                0,
                FW_EXTRABOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_TT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
                w!("Segoe UI"),
            );
            let old_font = SelectObject(hdc, font);

            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FF_FFFF));

            match layout {
                TrayLayout::Stacked | TrayLayout::ExtraLargeNumbersOnly => {
                    let mut r1 = RECT {
                        left: -1,
                        top: -2,
                        right: SIZE + 1,
                        bottom: SIZE / 2 + 2,
                    };
                    let mut r2 = RECT {
                        left: -1,
                        top: SIZE / 2 - 2,
                        right: SIZE + 1,
                        bottom: SIZE + 2,
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
            let _ = DeleteObject(font);
            SelectObject(hdc, old_bmp);
            let _ = DeleteObject(hbmp);
        }
        let _ = DeleteDC(hdc);
    }

    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("Valid RGBA icon")
}

// ============================================================================
// Floating Click-Through Taskbar Overlay Widget & Balloon Notification
// ============================================================================

fn create_overlay_window() -> HWND {
    unsafe {
        let hmodule = GetModuleHandleW(None).unwrap_or_default();
        let hinstance = HINSTANCE(hmodule.0);
        let class_name = w!("NetPulseOverlayClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name,
            w!("NetPulseOverlay"),
            WS_POPUP,
            0,
            0,
            OVERLAY_W,
            OVERLAY_H,
            None,
            None,
            hinstance,
            None,
        )
        .expect("Failed to create overlay window")
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn find_taskbar_overlay_position() -> (i32, i32) {
    unsafe {
        if let Ok(tray_hwnd) = FindWindowW(w!("Shell_TrayWnd"), None) {
            let mut tray_rect = RECT::default();
            if GetWindowRect(tray_hwnd, &mut tray_rect).is_ok() {
                let tray_h = tray_rect.bottom - tray_rect.top;
                let y = tray_rect.top + (tray_h - OVERLAY_H) / 2;

                if let Ok(notify_hwnd) =
                    FindWindowExW(tray_hwnd, None, w!("TrayNotifyWnd"), None)
                {
                    let mut notify_rect = RECT::default();
                    if GetWindowRect(notify_hwnd, &mut notify_rect).is_ok() {
                        return (notify_rect.left - OVERLAY_W - 6, y);
                    }
                }
                return (tray_rect.right - OVERLAY_W - 220, y);
            }
        }
    }
    (1600, 1032)
}

fn update_overlay_window(
    hwnd: HWND,
    show: bool,
    enabled: bool,
    down_bps: u64,
    up_bps: u64,
    rgb: (u8, u8, u8),
    limit_exceeded: bool,
) {
    unsafe {
        if !show {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }

        let (x, y) = find_taskbar_overlay_position();
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            OVERLAY_W,
            OVERLAY_H,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );

        let (line1, line2) = if !enabled {
            ("↓ Paused".to_string(), "↑ Paused".to_string())
        } else if limit_exceeded {
            (
                format!("LIMIT! ↓ {}/s", format_speed_long(down_bps)),
                format!("↑ {}/s", format_speed_long(up_bps)),
            )
        } else {
            (
                format!("↓ {}/s", format_speed_long(down_bps)),
                format!("↑ {}/s", format_speed_long(up_bps)),
            )
        };

        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: OVERLAY_W,
                biHeight: -OVERLAY_H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits_ptr: *mut c_void = std::ptr::null_mut();
        if let Ok(hbmp) = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0) {
            let old_bmp = SelectObject(mem_dc, hbmp);

            let bg_brush = CreateSolidBrush(COLORREF(0));
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: OVERLAY_W,
                bottom: OVERLAY_H,
            };
            FillRect(mem_dc, &full_rect, bg_brush);
            let _ = DeleteObject(bg_brush);

            let font = CreateFontW(
                -16,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_TT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
                w!("Segoe UI"),
            );
            let old_font = SelectObject(mem_dc, font);

            SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, COLORREF(0x00FF_FFFF));

            let mut r1 = RECT {
                left: 0,
                top: 2,
                right: OVERLAY_W - 4,
                bottom: OVERLAY_H / 2 + 1,
            };
            let mut r2 = RECT {
                left: 0,
                top: OVERLAY_H / 2 - 1,
                right: OVERLAY_W - 4,
                bottom: OVERLAY_H - 2,
            };

            let mut w1 = to_wide(&line1);
            let mut w2 = to_wide(&line2);
            let len1 = w1.len() - 1;
            let len2 = w2.len() - 1;

            DrawTextW(
                mem_dc,
                &mut w1[..len1],
                &mut r1,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            DrawTextW(
                mem_dc,
                &mut w2[..len2],
                &mut r2,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );

            let _ = GdiFlush();

            if !bits_ptr.is_null() {
                let pixels = std::slice::from_raw_parts_mut(
                    bits_ptr as *mut u8,
                    (OVERLAY_W * OVERLAY_H * 4) as usize,
                );
                for px in pixels.chunks_exact_mut(4) {
                    let cov = px[0].max(px[1]).max(px[2]);
                    let a = cov as u16;
                    px[0] = ((rgb.2 as u16 * a) / 255) as u8;
                    px[1] = ((rgb.1 as u16 * a) / 255) as u8;
                    px[2] = ((rgb.0 as u16 * a) / 255) as u8;
                    px[3] = cov;
                }
            }

            let pt_dst = POINT { x, y };
            let pt_src = POINT { x: 0, y: 0 };
            let sz = SIZE {
                cx: OVERLAY_W,
                cy: OVERLAY_H,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let _ = UpdateLayeredWindow(
                hwnd,
                HDC::default(),
                Some(&pt_dst),
                Some(&sz),
                mem_dc,
                Some(&pt_src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );

            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font);
            SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(hbmp);
        }

        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
    }
}

fn show_limit_balloon(hwnd: HWND, used_bytes: u64, limit_bytes: u64) {
    let title = "NetPulse — Data Limit Reached";
    let body = format!(
        "Session usage ({}) has exceeded your configured limit ({}).",
        format_bytes(used_bytes),
        format_bytes(limit_bytes)
    );

    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 99,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_WARNING,
        ..Default::default()
    };
    copy_to_utf16_buf(&mut nid.szInfoTitle, title);
    copy_to_utf16_buf(&mut nid.szInfo, &body);

    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn delete_limit_balloon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 99,
        ..Default::default()
    };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn copy_to_utf16_buf(buf: &mut [u16], text: &str) {
    let mut i = 0;
    for ch in text.encode_utf16() {
        if i + 1 >= buf.len() {
            break;
        }
        buf[i] = ch;
        i += 1;
    }
    if i < buf.len() {
        buf[i] = 0;
    }
}

// ============================================================================
// Lightweight %APPDATA%\NetPulse\config.ini Persistence (std::fs only)
// ============================================================================

fn config_file_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("NetPulse").join("config.ini"))
}

fn load_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    let Some(path) = config_file_path() else {
        return cfg;
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return cfg;
    };

    let mut parsed_rules = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "monitor_enabled" => cfg.monitor_enabled = v == "true",
            "layout" => {
                cfg.layout = match v {
                    "1" => TrayLayout::SideBySide,
                    "2" => TrayLayout::ExtraLargeNumbersOnly,
                    _ => TrayLayout::Stacked,
                }
            }
            "font_size" => {
                cfg.font_size = match v {
                    "0" => FontSizePreset::Small,
                    "2" => FontSizePreset::Large,
                    _ => FontSizePreset::Medium,
                }
            }
            "color_mode" => {
                cfg.color_mode = match v {
                    "1" => ColorMode::White,
                    "2" => ColorMode::Black,
                    "3" => ColorMode::Cyan,
                    "4" => ColorMode::Green,
                    "5" => ColorMode::Amber,
                    _ => ColorMode::Auto,
                }
            }
            "show_overlay" => cfg.show_overlay = v == "true",
            "dynamic_enabled" => cfg.dynamic_enabled = v == "true",
            "dynamic_metric" => {
                cfg.dynamic_metric = if v == "1" {
                    SpeedMetric::Total
                } else {
                    SpeedMetric::Download
                }
            }
            "limit_enabled" => cfg.limit_enabled = v == "true",
            "limit_value" => {
                if let Ok(val) = v.parse::<f64>() {
                    cfg.limit_value = val.max(0.1);
                }
            }
            "limit_unit" => {
                cfg.limit_unit = if v == "0" { DataUnit::MB } else { DataUnit::GB };
            }
            "rule" => {
                let parts: Vec<&str> = v.split(',').collect();
                if parts.len() == 9 {
                    let min_val = parts[0].parse::<f64>().unwrap_or(0.0);
                    let min_unit = if parts[1] == "2" {
                        SpeedUnit::MBps
                    } else {
                        SpeedUnit::KBps
                    };
                    let max_val = parts[2].parse::<f64>().unwrap_or(0.0);
                    let max_unit = match parts[3] {
                        "1" => SpeedUnit::KBps,
                        "2" => SpeedUnit::MBps,
                        _ => SpeedUnit::Unlimited,
                    };
                    let is_auto = parts[4] == "1";
                    let r = parts[5].parse::<u8>().unwrap_or(255);
                    let g = parts[6].parse::<u8>().unwrap_or(255);
                    let b = parts[7].parse::<u8>().unwrap_or(255);
                    let color_label = parts[8].to_string();
                    parsed_rules.push(ColorRule {
                        min_val,
                        min_unit,
                        max_val,
                        max_unit,
                        is_auto,
                        rgb: (r, g, b),
                        color_label,
                    });
                }
            }
            _ => {}
        }
    }

    if !parsed_rules.is_empty() {
        cfg.rules = parsed_rules;
    }
    cfg
}

fn save_config(cfg: &AppConfig) {
    let Some(path) = config_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut out = String::new();
    out.push_str(&format!("monitor_enabled={}\n", cfg.monitor_enabled));
    out.push_str(&format!("layout={}\n", cfg.layout as usize));
    out.push_str(&format!("font_size={}\n", cfg.font_size as usize));
    out.push_str(&format!("color_mode={}\n", cfg.color_mode as usize));
    out.push_str(&format!("show_overlay={}\n", cfg.show_overlay));
    out.push_str(&format!("dynamic_enabled={}\n", cfg.dynamic_enabled));
    out.push_str(&format!("dynamic_metric={}\n", cfg.dynamic_metric as usize));
    out.push_str(&format!("limit_enabled={}\n", cfg.limit_enabled));
    out.push_str(&format!("limit_value={}\n", cfg.limit_value));
    out.push_str(&format!("limit_unit={}\n", cfg.limit_unit as usize));

    for rule in &cfg.rules {
        let clean_label = rule.color_label.replace(',', " ");
        out.push_str(&format!(
            "rule={},{},{},{},{},{},{},{},{}\n",
            rule.min_val,
            rule.min_unit as usize,
            rule.max_val,
            rule.max_unit as usize,
            if rule.is_auto { 1 } else { 0 },
            rule.rgb.0,
            rule.rgb.1,
            rule.rgb.2,
            clean_label
        ));
    }

    let _ = fs::write(path, out);
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
    overlay_hwnd: HWND,
) -> SettingsWindow {
    unsafe {
        let hmodule = GetModuleHandleW(None).unwrap_or_default();
        let hinstance = HINSTANCE(hmodule.0);
        let class_name = w!("NetPulseSettingsClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(settings_wnd_proc),
            hInstance: hinstance,
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
            470,
            685,
            None,
            None,
            hinstance,
            None,
        )
        .expect("Failed to create settings window");

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
            DEFAULT_CHARSET.0 as u32,
            OUT_TT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
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
                hwnd,
                HMENU(id as *mut c_void),
                hinstance,
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

        // Live telemetry labels
        let lbl_down = make_ctrl(w!("STATIC"), w!("Download:  ↓ 0 B/s"), SS_LEFT_STYLE, 18, 14, 420, 20, 0);
        let lbl_up = make_ctrl(w!("STATIC"), w!("Upload:    ↑ 0 B/s"), SS_LEFT_STYLE, 18, 36, 420, 20, 0);
        let lbl_session = make_ctrl(w!("STATIC"), w!("Session:   ↓ 0 B   |   ↑ 0 B"), SS_LEFT_STYLE, 18, 58, 420, 20, 0);
        let lbl_adapter = make_ctrl(w!("STATIC"), w!("Adapter:   Detecting..."), SS_LEFT_STYLE, 18, 80, 420, 20, 0);

        // General settings
        let chk_enabled = make_ctrl(
            w!("BUTTON"),
            w!("Enable Taskbar Speed Monitor"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            18,
            108,
            240,
            22,
            ID_CHK_ENABLED,
        );
        let chk_overlay = make_ctrl(
            w!("BUTTON"),
            w!("Show Large Taskbar Overlay (Next to Clock)"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            18,
            134,
            340,
            22,
            ID_CHK_OVERLAY,
        );
        let chk_autostart = make_ctrl(
            w!("BUTTON"),
            w!("Start automatically with Windows"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            18,
            160,
            280,
            22,
            ID_CHK_AUTOSTART,
        );

        let _ = make_ctrl(w!("STATIC"), w!("Tray Icon Layout:"), SS_LEFT_STYLE, 18, 194, 125, 20, 0);
        let cmb_layout = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            150,
            190,
            285,
            140,
            ID_CMB_LAYOUT,
        );
        for opt in [
            w!("Stacked (↓ / ↑)"),
            w!("Side-by-Side (↓ ↑)"),
            w!("Extra Large (Numbers Only)"),
        ] {
            SendMessageW(cmb_layout, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }

        let _ = make_ctrl(w!("STATIC"), w!("Font Size:"), SS_LEFT_STYLE, 18, 224, 125, 20, 0);
        let cmb_fontsize = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            150,
            220,
            120,
            120,
            ID_CMB_FONTSIZE,
        );
        for opt in [w!("Small"), w!("Medium"), w!("Large")] {
            SendMessageW(cmb_fontsize, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }

        let _ = make_ctrl(w!("STATIC"), w!("Static Color:"), SS_LEFT_STYLE, 282, 224, 80, 20, 0);
        let cmb_color = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            365,
            220,
            70,
            160,
            ID_CMB_COLOR,
        );
        for opt in [
            w!("Auto"),
            w!("White"),
            w!("Black"),
            w!("Cyan"),
            w!("Green"),
            w!("Amber"),
        ] {
            SendMessageW(cmb_color, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }

        // Dynamic Speed Colors Section
        let _ = make_ctrl(
            w!("STATIC"),
            w!("─── Dynamic Speed Colors ─────────────────────────────"),
            SS_LEFT_STYLE,
            18,
            258,
            420,
            18,
            0,
        );
        let chk_dyn_enabled = make_ctrl(
            w!("BUTTON"),
            w!("Enable Dynamic Color Rules"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            18,
            280,
            200,
            22,
            ID_CHK_DYN_ENABLED,
        );
        let _ = make_ctrl(w!("STATIC"), w!("Measure:"), SS_LEFT_STYLE, 228, 282, 58, 20, 0);
        let cmb_dyn_metric = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            290,
            278,
            145,
            100,
            ID_CMB_DYN_METRIC,
        );
        for opt in [w!("Download Speed"), w!("Total (↓ + ↑)")] {
            SendMessageW(cmb_dyn_metric, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }

        let lst_rules = make_ctrl(
            w!("LISTBOX"),
            w!(""),
            LBS_NOTIFY_STYLE | WS_VSCROLL.0 | WS_BORDER.0 | WS_TABSTOP.0,
            18,
            308,
            417,
            95,
            ID_LST_RULES,
        );

        let _ = make_ctrl(w!("STATIC"), w!("Min:"), SS_LEFT_STYLE, 18, 412, 32, 20, 0);
        let edt_min = make_ctrl(
            w!("EDIT"),
            w!("0"),
            ES_AUTOHSCROLL_STYLE | WS_BORDER.0 | WS_TABSTOP.0,
            52,
            410,
            58,
            22,
            ID_EDT_MIN,
        );
        let cmb_min_unit = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            114,
            409,
            66,
            100,
            ID_CMB_MIN_UNIT,
        );
        for opt in [w!("KB/s"), w!("MB/s")] {
            SendMessageW(cmb_min_unit, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }
        SendMessageW(cmb_min_unit, CB_SETCURSEL, WPARAM(0), LPARAM(0));

        let _ = make_ctrl(w!("STATIC"), w!("Max:"), SS_LEFT_STYLE, 192, 412, 32, 20, 0);
        let edt_max = make_ctrl(
            w!("EDIT"),
            w!("500"),
            ES_AUTOHSCROLL_STYLE | WS_BORDER.0 | WS_TABSTOP.0,
            226,
            410,
            58,
            22,
            ID_EDT_MAX,
        );
        let cmb_max_unit = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            288,
            409,
            95,
            120,
            ID_CMB_MAX_UNIT,
        );
        for opt in [w!("Unlimited"), w!("KB/s"), w!("MB/s")] {
            SendMessageW(cmb_max_unit, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }
        SendMessageW(cmb_max_unit, CB_SETCURSEL, WPARAM(1), LPARAM(0));

        let _ = make_ctrl(w!("STATIC"), w!("Color:"), SS_LEFT_STYLE, 18, 444, 42, 20, 0);
        let cmb_rule_color = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            64,
            440,
            125,
            220,
            ID_CMB_RULE_COLOR,
        );
        for (name, _, _) in preset_color_options() {
            let wname = to_wide(name);
            SendMessageW(
                cmb_rule_color,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(wname.as_ptr() as isize),
            );
        }
        SendMessageW(cmb_rule_color, CB_SETCURSEL, WPARAM(4), LPARAM(0));

        let _ = make_ctrl(w!("STATIC"), w!("Hex:"), SS_LEFT_STYLE, 198, 444, 30, 20, 0);
        let edt_rule_hex = make_ctrl(
            w!("EDIT"),
            w!("#4ADE80"),
            ES_AUTOHSCROLL_STYLE | WS_BORDER.0 | WS_TABSTOP.0,
            232,
            441,
            78,
            22,
            ID_EDT_RULE_HEX,
        );
        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Pick Color..."),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            318,
            440,
            117,
            24,
            ID_BTN_PICK_COLOR,
        );

        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Add Rule"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            18,
            472,
            130,
            26,
            ID_BTN_ADD_RULE,
        );
        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Update Selected"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            161,
            472,
            130,
            26,
            ID_BTN_UPD_RULE,
        );
        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Delete Selected"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            305,
            472,
            130,
            26,
            ID_BTN_DEL_RULE,
        );

        // Data Usage Limit Section
        let _ = make_ctrl(
            w!("STATIC"),
            w!("─── Data Usage Limit (Session) ───────────────────────"),
            SS_LEFT_STYLE,
            18,
            510,
            420,
            18,
            0,
        );
        let chk_limit_enabled = make_ctrl(
            w!("BUTTON"),
            w!("Enable Data Limit Alert"),
            BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
            18,
            534,
            185,
            22,
            ID_CHK_LIMIT_ENABLED,
        );
        let _ = make_ctrl(w!("STATIC"), w!("Limit:"), SS_LEFT_STYLE, 215, 536, 38, 20, 0);
        let edt_limit_val = make_ctrl(
            w!("EDIT"),
            w!("1"),
            ES_AUTOHSCROLL_STYLE | WS_BORDER.0 | WS_TABSTOP.0,
            258,
            534,
            70,
            22,
            ID_EDT_LIMIT_VAL,
        );
        let cmb_limit_unit = make_ctrl(
            w!("COMBOBOX"),
            w!(""),
            CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0,
            335,
            533,
            100,
            100,
            ID_CMB_LIMIT_UNIT,
        );
        for opt in [w!("MB"), w!("GB")] {
            SendMessageW(cmb_limit_unit, CB_ADDSTRING, WPARAM(0), LPARAM(opt.as_ptr() as isize));
        }

        // Bottom Actions
        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Reset Session & Alert"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            18,
            584,
            190,
            32,
            ID_BTN_RESET,
        );
        let _ = make_ctrl(
            w!("BUTTON"),
            w!("Hide to Tray"),
            BS_PUSHBUTTON as u32 | WS_TABSTOP.0,
            245,
            584,
            190,
            32,
            ID_BTN_HIDE,
        );

        let user_data = Box::new(WindowUserData {
            shared,
            proxy,
            chk_enabled,
            chk_autostart,
            chk_overlay,
            cmb_layout,
            cmb_fontsize,
            cmb_color,
            chk_dyn_enabled,
            cmb_dyn_metric,
            lst_rules,
            edt_min,
            cmb_min_unit,
            edt_max,
            cmb_max_unit,
            cmb_rule_color,
            edt_rule_hex,
            chk_limit_enabled,
            edt_limit_val,
            cmb_limit_unit,
            overlay_hwnd,
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
            chk_overlay,
            cmb_layout,
            cmb_fontsize,
            cmb_color,
            chk_dyn_enabled,
            cmb_dyn_metric,
            lst_rules,
            chk_limit_enabled,
            edt_limit_val,
            cmb_limit_unit,
            ui_font,
        }
    }
}

unsafe fn populate_rules_listbox(lst: HWND, rules: &[ColorRule]) {
    let prev_sel = SendMessageW(lst, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    SendMessageW(lst, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    for rule in rules {
        let wtext = to_wide(&rule.display_text());
        SendMessageW(lst, LB_ADDSTRING, WPARAM(0), LPARAM(wtext.as_ptr() as isize));
    }
    if prev_sel >= 0 && (prev_sel as usize) < rules.len() {
        SendMessageW(lst, LB_SETCURSEL, WPARAM(prev_sel as usize), LPARAM(0));
    }
}

unsafe fn get_edit_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize]).trim().to_string()
}

unsafe fn read_rule_from_inputs(data: &WindowUserData) -> ColorRule {
    let min_val = get_edit_text(data.edt_min).parse::<f64>().unwrap_or(0.0).max(0.0);
    let min_unit_idx = SendMessageW(data.cmb_min_unit, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let min_unit = if min_unit_idx == 1 {
        SpeedUnit::MBps
    } else {
        SpeedUnit::KBps
    };

    let max_val = get_edit_text(data.edt_max).parse::<f64>().unwrap_or(0.0).max(0.0);
    let max_unit_idx = SendMessageW(data.cmb_max_unit, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let max_unit = match max_unit_idx {
        1 => SpeedUnit::KBps,
        2 => SpeedUnit::MBps,
        _ => SpeedUnit::Unlimited,
    };

    let col_idx = SendMessageW(data.cmb_rule_color, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let presets = preset_color_options();
    let (preset_name, is_auto, preset_rgb) = if col_idx >= 0 && (col_idx as usize) < presets.len()
    {
        presets[col_idx as usize]
    } else {
        ("Custom", false, (255, 255, 255))
    };

    let hex_str = get_edit_text(data.edt_rule_hex);
    let rgb = parse_hex_rgb(&hex_str).unwrap_or(preset_rgb);

    ColorRule {
        min_val,
        min_unit,
        max_val,
        max_unit,
        is_auto,
        rgb,
        color_label: preset_name.to_string(),
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
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowUserData;
            if !ptr.is_null() {
                let data = &*ptr;
                let _ = data.proxy.send_event(UserEvent::SettingsChanged);
            }
            let _ = InvalidateRect(hwnd, None, true);
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
                                as usize
                                == BST_CHECKED_VAL;
                        data.shared.lock().unwrap().config.monitor_enabled = checked;
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CHK_OVERLAY => {
                        let checked =
                            SendMessageW(data.chk_overlay, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
                                as usize
                                == BST_CHECKED_VAL;
                        data.shared.lock().unwrap().config.show_overlay = checked;
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CHK_AUTOSTART => {
                        let checked =
                            SendMessageW(data.chk_autostart, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
                                as usize
                                == BST_CHECKED_VAL;
                        if set_autostart(checked).is_ok() {
                            data.shared.lock().unwrap().config.auto_start = checked;
                        }
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_LAYOUT if notif == CBN_SELCHANGE_NOTIF => {
                        let idx =
                            SendMessageW(data.cmb_layout, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        data.shared.lock().unwrap().config.layout = match idx {
                            1 => TrayLayout::SideBySide,
                            2 => TrayLayout::ExtraLargeNumbersOnly,
                            _ => TrayLayout::Stacked,
                        };
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_FONTSIZE if notif == CBN_SELCHANGE_NOTIF => {
                        let idx =
                            SendMessageW(data.cmb_fontsize, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        data.shared.lock().unwrap().config.font_size = match idx {
                            0 => FontSizePreset::Small,
                            2 => FontSizePreset::Large,
                            _ => FontSizePreset::Medium,
                        };
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_COLOR if notif == CBN_SELCHANGE_NOTIF => {
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
                    ID_CHK_DYN_ENABLED => {
                        let checked =
                            SendMessageW(data.chk_dyn_enabled, BM_GETCHECK, WPARAM(0), LPARAM(0)).0
                                as usize
                                == BST_CHECKED_VAL;
                        data.shared.lock().unwrap().config.dynamic_enabled = checked;
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_DYN_METRIC if notif == CBN_SELCHANGE_NOTIF => {
                        let idx =
                            SendMessageW(data.cmb_dyn_metric, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        data.shared.lock().unwrap().config.dynamic_metric = if idx == 1 {
                            SpeedMetric::Total
                        } else {
                            SpeedMetric::Download
                        };
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_LST_RULES if notif == LBN_SELCHANGE_NOTIF => {
                        let sel = SendMessageW(data.lst_rules, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        if sel >= 0 {
                            let state = data.shared.lock().unwrap();
                            if let Some(rule) = state.config.rules.get(sel as usize) {
                                let wmin = to_wide(&trim_num(rule.min_val));
                                let _ = SetWindowTextW(data.edt_min, PCWSTR(wmin.as_ptr()));
                                SendMessageW(
                                    data.cmb_min_unit,
                                    CB_SETCURSEL,
                                    WPARAM(if rule.min_unit == SpeedUnit::MBps { 1 } else { 0 }),
                                    LPARAM(0),
                                );

                                let wmax = to_wide(&trim_num(rule.max_val));
                                let _ = SetWindowTextW(data.edt_max, PCWSTR(wmax.as_ptr()));
                                SendMessageW(
                                    data.cmb_max_unit,
                                    CB_SETCURSEL,
                                    WPARAM(rule.max_unit as usize),
                                    LPARAM(0),
                                );

                                let whex = to_wide(&format!(
                                    "#{:02X}{:02X}{:02X}",
                                    rule.rgb.0, rule.rgb.1, rule.rgb.2
                                ));
                                let _ = SetWindowTextW(data.edt_rule_hex, PCWSTR(whex.as_ptr()));

                                let presets = preset_color_options();
                                let found = presets
                                    .iter()
                                    .position(|(name, _, _)| *name == rule.color_label)
                                    .unwrap_or(presets.len() - 1);
                                SendMessageW(data.cmb_rule_color, CB_SETCURSEL, WPARAM(found), LPARAM(0));
                            }
                        }
                    }
                    ID_CMB_RULE_COLOR if notif == CBN_SELCHANGE_NOTIF => {
                        let idx =
                            SendMessageW(data.cmb_rule_color, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        let presets = preset_color_options();
                        if idx >= 0 && (idx as usize) < presets.len() - 1 {
                            let (_, _, rgb) = presets[idx as usize];
                            let whex = to_wide(&format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2));
                            let _ = SetWindowTextW(data.edt_rule_hex, PCWSTR(whex.as_ptr()));
                        }
                    }
                    ID_BTN_PICK_COLOR => {
                        let cur_rgb = parse_hex_rgb(&get_edit_text(data.edt_rule_hex))
                            .unwrap_or((74, 222, 128));
                        let bgr = ((cur_rgb.2 as u32) << 16)
                            | ((cur_rgb.1 as u32) << 8)
                            | (cur_rgb.0 as u32);
                        let mut cust_colors = [COLORREF(0x00FF_FFFF); 16];
                        let mut cc = CHOOSECOLORW {
                            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
                            hwndOwner: hwnd,
                            rgbResult: COLORREF(bgr),
                            lpCustColors: cust_colors.as_mut_ptr(),
                            Flags: CC_FULLOPEN | CC_RGBINIT,
                            ..Default::default()
                        };
                        if ChooseColorW(&mut cc).as_bool() {
                            let r = (cc.rgbResult.0 & 0xFF) as u8;
                            let g = ((cc.rgbResult.0 >> 8) & 0xFF) as u8;
                            let b = ((cc.rgbResult.0 >> 16) & 0xFF) as u8;
                            let whex = to_wide(&format!("#{:02X}{:02X}{:02X}", r, g, b));
                            let _ = SetWindowTextW(data.edt_rule_hex, PCWSTR(whex.as_ptr()));
                            let custom_idx = preset_color_options().len() - 1;
                            SendMessageW(
                                data.cmb_rule_color,
                                CB_SETCURSEL,
                                WPARAM(custom_idx),
                                LPARAM(0),
                            );
                        }
                    }
                    ID_BTN_ADD_RULE => {
                        let rule = read_rule_from_inputs(data);
                        data.shared.lock().unwrap().config.rules.push(rule);
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_BTN_UPD_RULE => {
                        let sel = SendMessageW(data.lst_rules, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        if sel >= 0 {
                            let rule = read_rule_from_inputs(data);
                            let mut state = data.shared.lock().unwrap();
                            if let Some(slot) = state.config.rules.get_mut(sel as usize) {
                                *slot = rule;
                            }
                            drop(state);
                            let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                        }
                    }
                    ID_BTN_DEL_RULE => {
                        let sel = SendMessageW(data.lst_rules, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        if sel >= 0 {
                            let mut state = data.shared.lock().unwrap();
                            if (sel as usize) < state.config.rules.len() {
                                state.config.rules.remove(sel as usize);
                            }
                            drop(state);
                            let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                        }
                    }
                    ID_CHK_LIMIT_ENABLED => {
                        let checked =
                            SendMessageW(data.chk_limit_enabled, BM_GETCHECK, WPARAM(0), LPARAM(0))
                                .0 as usize
                                == BST_CHECKED_VAL;
                        let lim_val = get_edit_text(data.edt_limit_val)
                            .parse::<f64>()
                            .unwrap_or(1.0)
                            .max(0.1);
                        let unit_idx =
                            SendMessageW(data.cmb_limit_unit, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        let mut state = data.shared.lock().unwrap();
                        state.config.limit_enabled = checked;
                        state.config.limit_value = lim_val;
                        state.config.limit_unit = if unit_idx == 0 {
                            DataUnit::MB
                        } else {
                            DataUnit::GB
                        };
                        state.stats.alert_triggered = false;
                        drop(state);
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_CMB_LIMIT_UNIT if notif == CBN_SELCHANGE_NOTIF => {
                        let lim_val = get_edit_text(data.edt_limit_val)
                            .parse::<f64>()
                            .unwrap_or(1.0)
                            .max(0.1);
                        let unit_idx =
                            SendMessageW(data.cmb_limit_unit, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                        let mut state = data.shared.lock().unwrap();
                        state.config.limit_value = lim_val;
                        state.config.limit_unit = if unit_idx == 0 {
                            DataUnit::MB
                        } else {
                            DataUnit::GB
                        };
                        state.stats.alert_triggered = false;
                        drop(state);
                        let _ = data.proxy.send_event(UserEvent::SettingsChanged);
                    }
                    ID_EDT_LIMIT_VAL if notif == 0x0300 => {
                        // EN_CHANGE
                        if let Ok(lim_val) = get_edit_text(data.edt_limit_val).parse::<f64>() {
                            if lim_val > 0.0 {
                                let mut state = data.shared.lock().unwrap();
                                state.config.limit_value = lim_val;
                                state.stats.alert_triggered = false;
                            }
                        }
                    }
                    ID_BTN_RESET => {
                        let mut state = data.shared.lock().unwrap();
                        state.stats.session_down_bytes = 0;
                        state.stats.session_up_bytes = 0;
                        state.stats.alert_triggered = false;
                        drop(state);
                        delete_limit_balloon(data.overlay_hwnd);
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
