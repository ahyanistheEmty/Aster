#![windows_subsystem = "windows"]

use std::{mem, sync::mpsc};

use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::*,
    Win32::{
        Foundation::{COLORREF, E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
        },
        Graphics::Gdi::{
            self, BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
            CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW,
            GetStockObject, InvalidateRect, LineTo, MonitorFromWindow, MoveToEx, RoundRect, ScreenToClient,
            SelectObject, SetBkMode, SetTextColor, SRCCOPY, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE,
            DT_VCENTER, HBRUSH, HDC, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, NULL_BRUSH,
            NULL_PEN, TRANSPARENT,
        },
        System::{Com::*, LibraryLoader},
        UI::{
            Controls::{EM_SETMARGINS, EM_SETSEL},
            HiDpi,
            Input::KeyboardAndMouse::{
                GetKeyState, SetFocus, VK_CONTROL, VK_F11, VK_F5, VK_MENU, VK_RETURN,
            },
            WindowsAndMessaging::{
                self, CREATESTRUCTW, CW_USEDEFAULT, EC_LEFTMARGIN, EC_RIGHTMARGIN, GetCursorPos, GWLP_USERDATA,
                GWLP_WNDPROC, GWL_STYLE, HMENU, HWND_TOP, ICON_BIG, ICON_SMALL, IDC_ARROW, MSG,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP, WM_CHAR, WM_CLOSE,
                WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC,
                WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT,
                WM_SETCURSOR, WM_SETFOCUS, WM_SETFONT, WM_SETICON, WM_SIZE, WM_TIMER, WNDCLASSW,
                WNDPROC, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_DLGMODALFRAME,
                WS_OVERLAPPEDWINDOW, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
};

const APP_NAME: PCWSTR = w!("Aster");
const CLASS_NAME: PCWSTR = w!("AsterWindow");
const ADDRESS_ID: i32 = 1001;
const DEFAULT_URL: &str = "https://www.google.com";
const SIDEBAR_EXPANDED: f32 = 248.0;
const SIDEBAR_HIDDEN: f32 = 0.0;
const HOVER_ZONE: i32 = 8;
const TOPBAR_HEIGHT: i32 = 58;
const TAB_HEIGHT: i32 = 48;
const TAB_TOP: i32 = 76;
const SIDEBAR_TIMER_ID: usize = 42;
const HOVER_LEAVE_TIMER_ID: usize = 43;

const COLOR_BLACK: u32 = 0x000000;
const COLOR_PANEL: u32 = 0x090909;
const COLOR_PANEL_2: u32 = 0x121212;
const COLOR_SURFACE: u32 = 0x161616;
const COLOR_SURFACE_HOVER: u32 = 0x242424;
const COLOR_BORDER: u32 = 0x343434;
const COLOR_TEXT: u32 = 0xf5f5f5;
const COLOR_MUTED: u32 = 0xa1a1a1;
const COLOR_ACCENT: u32 = 0xf16f63;

static mut OLD_ADDRESS_PROC: WNDPROC = None;

type AppResult<T> = std::result::Result<T, AppError>;

#[derive(Debug)]
enum AppError {
    Windows(windows::core::Error),
    WebView(webview2_com::Error),
    Channel,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Windows(error) => write!(f, "Windows error: {error}"),
            Self::WebView(error) => write!(f, "WebView2 error: {error}"),
            Self::Channel => write!(f, "startup channel closed"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<windows::core::Error> for AppError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}

impl From<HRESULT> for AppError {
    fn from(value: HRESULT) -> Self {
        Self::Windows(windows::core::Error::from(value))
    }
}

impl From<webview2_com::Error> for AppError {
    fn from(value: webview2_com::Error) -> Self {
        Self::WebView(value)
    }
}

impl From<mpsc::RecvError> for AppError {
    fn from(_: mpsc::RecvError) -> Self {
        Self::Channel
    }
}

struct Tab {
    id: usize,
    title: String,
    url: String,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoverTarget {
    Logo,
    NewTab,
    Back,
    Forward,
    Reload,
    Settings,
    ModeRow,
    ModeAuto,
    ModeDark,
    ModeLight,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarMode {
    Hidden,
    Overlay,
    Pushed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SiteMode {
    Auto,
    Dark,
    Light,
}

impl SiteMode {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    fn scheme(self) -> COREWEBVIEW2_PREFERRED_COLOR_SCHEME {
        match self {
            Self::Auto => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
            Self::Dark => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            Self::Light => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
        }
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

struct UiFonts {
    body: HFONT,
    small: HFONT,
    icon: HFONT,
}

impl Drop for UiFonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.body.0));
            let _ = DeleteObject(HGDIOBJ(self.small.0));
            let _ = DeleteObject(HGDIOBJ(self.icon.0));
        }
    }
}

struct UiBrushes {
    black: HBRUSH,
    panel: HBRUSH,
    panel_2: HBRUSH,
    edit: HBRUSH,
}

impl Drop for UiBrushes {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.black.0));
            let _ = DeleteObject(HGDIOBJ(self.panel.0));
            let _ = DeleteObject(HGDIOBJ(self.panel_2.0));
            let _ = DeleteObject(HGDIOBJ(self.edit.0));
        }
    }
}

struct App {
    hwnd: HWND,
    address_hwnd: HWND,
    environment: ICoreWebView2Environment,
    tabs: Vec<Tab>,
    active: usize,
    next_id: usize,
    fonts: UiFonts,
    brushes: UiBrushes,
    hover_close: Option<usize>,
    hover_tab: Option<usize>,
    hover_target: Option<HoverTarget>,
    sidebar_width: f32,
    sidebar_target: f32,
    sidebar_mode: SidebarMode,
    sidebar_expand_mode: SidebarMode,
    animating_sidebar: bool,
    hovering_sidebar: bool,
    site_mode: SiteMode,
    settings_open: bool,
    mode_menu_open: bool,
    fullscreen: bool,
    saved_style: isize,
    saved_rect: RECT,
}

impl App {
    fn new(hwnd: HWND, environment: ICoreWebView2Environment) -> AppResult<Self> {
        let fonts = UiFonts {
            body: create_font(14, 400)?,
            small: create_font(12, 400)?,
            icon: create_font_with_face(18, 400, w!("Segoe Fluent Icons"))?,
        };
        let brushes = UiBrushes {
            black: solid_brush(COLOR_BLACK),
            panel: solid_brush(COLOR_PANEL),
            panel_2: solid_brush(COLOR_PANEL_2),
            edit: solid_brush(0x151515),
        };

        let address_hwnd = create_address_bar(hwnd)?;
        unsafe {
            let _ = WindowsAndMessaging::SendMessageW(
                address_hwnd,
                WM_SETFONT,
                Some(WPARAM(fonts.body.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        let mut app = Self {
            hwnd,
            address_hwnd,
            environment,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            fonts,
            brushes,
            hover_close: None,
            hover_tab: None,
            hover_target: None,
            sidebar_width: SIDEBAR_HIDDEN,
            sidebar_target: SIDEBAR_HIDDEN,
            sidebar_mode: SidebarMode::Hidden,
            sidebar_expand_mode: SidebarMode::Hidden,
            animating_sidebar: false,
            hovering_sidebar: false,
            site_mode: SiteMode::Auto,
            settings_open: false,
            mode_menu_open: false,
            fullscreen: false,
            saved_style: 0,
            saved_rect: RECT::default(),
        };
        app.create_tab(DEFAULT_URL)?;
        Ok(app)
    }

    fn create_tab(&mut self, url: &str) -> AppResult<()> {
        let controller = create_webview_controller(&self.environment, self.hwnd)?;
        let webview = unsafe { controller.CoreWebView2()? };
        configure_webview(&webview)?;
        apply_site_mode_to_webview(&webview, self.site_mode);

        let id = self.next_id;
        self.next_id += 1;
        let index = self.tabs.len();
        self.attach_events(index, id, &webview)?;
        self.attach_controller_events(&controller)?;

        unsafe {
            controller.SetIsVisible(false)?;
        }
        self.tabs.push(Tab {
            id,
            title: "New Tab".to_string(),
            url: url.to_string(),
            controller,
            webview,
        });
        self.switch_to(index);
        self.navigate_active(url);
        Ok(())
    }

    fn attach_controller_events(&self, controller: &ICoreWebView2Controller) -> AppResult<()> {
        unsafe {
            let hwnd = self.hwnd;
            let mut token = 0;
            controller.add_AcceleratorKeyPressed(
                &AcceleratorKeyPressedEventHandler::create(Box::new(move |_sender, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND(0);
                        let mut key = 0;
                        if args.KeyEventKind(&mut kind).is_ok()
                            && args.VirtualKey(&mut key).is_ok()
                            && (kind.0 == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN.0
                                || kind.0 == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN.0)
                            && is_aster_shortcut(key)
                        {
                            handle_keydown(hwnd, WPARAM(key as usize));
                            let _ = args.SetHandled(true);
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(())
    }

    fn attach_events(
        &self,
        index_hint: usize,
        tab_id: usize,
        webview: &ICoreWebView2,
    ) -> AppResult<()> {
        unsafe {
            let hwnd = self.hwnd;
            let mut token = 0;
            webview.add_DocumentTitleChanged(
                &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _args| {
                    if let Some(sender) = sender {
                        let mut title = PWSTR::null();
                        if sender.DocumentTitle(&mut title).is_ok() {
                            let title = CoTaskMemPWSTR::from(title).to_string();
                            with_app(hwnd, |app| app.update_tab_title(tab_id, title));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let hwnd = self.hwnd;
            let mut token = 0;
            webview.add_SourceChanged(
                &SourceChangedEventHandler::create(Box::new(move |sender, _args| {
                    if let Some(sender) = sender {
                        let mut uri = PWSTR::null();
                        if sender.Source(&mut uri).is_ok() {
                            let url = CoTaskMemPWSTR::from(uri).to_string();
                            with_app(hwnd, |app| app.update_tab_url(tab_id, url));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let hwnd = self.hwnd;
            let mut token = 0;
            webview.add_HistoryChanged(
                &HistoryChangedEventHandler::create(Box::new(move |_sender, _args| {
                    with_app(hwnd, |app| app.refresh());
                    Ok(())
                })),
                &mut token,
            )?;

            let hwnd = self.hwnd;
            let mut token = 0;
            webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |_sender, args| {
                    if let Some(args) = args {
                        let mut uri = PWSTR::null();
                        if args.Uri(&mut uri).is_ok() {
                            let url = CoTaskMemPWSTR::from(uri).to_string();
                            with_app(hwnd, |app| {
                                let _ = app.create_tab(&url);
                            });
                            let _ = args.SetHandled(true);
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }

        if index_hint == usize::MAX {
            unreachable!();
        }
        Ok(())
    }

    fn update_tab_title(&mut self, tab_id: usize, title: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            let trimmed = title.trim();
            if !trimmed.is_empty() {
                tab.title = trimmed.to_string();
            }
        }
        self.refresh();
    }

    fn update_tab_url(&mut self, tab_id: usize, url: String) {
        if let Some((index, tab)) = self
            .tabs
            .iter_mut()
            .enumerate()
            .find(|(_, tab)| tab.id == tab_id)
        {
            tab.url = if url == "about:blank" {
                String::new()
            } else {
                url
            };
            if index == self.active {
                set_window_text(self.address_hwnd, &tab.url);
            }
        }
        self.refresh();
    }

    fn switch_to(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            unsafe {
                let _ = tab.controller.SetIsVisible(i == index);
            }
        }
        self.active = index;
        self.layout();
        if let Some(tab) = self.tabs.get(index) {
            set_window_text(self.address_hwnd, &tab.url);
            unsafe {
                let _ = tab
                    .controller
                    .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
        self.refresh();
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.is_empty() || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            let _ = self.create_tab(DEFAULT_URL);
            return;
        }
        let next = if index >= self.tabs.len() {
            self.tabs.len() - 1
        } else {
            index
        };
        self.switch_to(next);
    }

    fn navigate_active_from_address(&mut self) {
        let raw = get_window_text(self.address_hwnd);
        let url = normalize_address(&raw);
        self.navigate_active(&url);
    }

    fn navigate_active(&mut self, url: &str) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.url = url.to_string();
            tab.title = label_for_url(url);
            set_window_text(self.address_hwnd, url);
            let wide = CoTaskMemPWSTR::from(url);
            unsafe {
                let _ = tab.webview.Navigate(*wide.as_ref().as_pcwstr());
            }
        }
        self.refresh();
    }

    fn go_back(&self) {
        if let Some(tab) = self.tabs.get(self.active) {
            unsafe {
                let mut can = BOOL::from(false);
                if tab.webview.CanGoBack(&mut can).is_ok() && can.as_bool() {
                    let _ = tab.webview.GoBack();
                }
            }
        }
    }

    fn go_forward(&self) {
        if let Some(tab) = self.tabs.get(self.active) {
            unsafe {
                let mut can = BOOL::from(false);
                if tab.webview.CanGoForward(&mut can).is_ok() && can.as_bool() {
                    let _ = tab.webview.GoForward();
                }
            }
        }
    }

    fn reload(&self) {
        if let Some(tab) = self.tabs.get(self.active) {
            unsafe {
                let _ = tab.webview.Reload();
            }
        }
    }

    fn sidebar_width(&self) -> i32 {
        self.sidebar_width.round() as i32
    }

    fn top_button_x(&self) -> i32 {
        match self.sidebar_mode {
            SidebarMode::Hidden => {
                if self.sidebar_target >= SIDEBAR_EXPANDED {
                    match self.sidebar_expand_mode {
                        SidebarMode::Overlay => 0,
                        SidebarMode::Pushed => self.sidebar_width(),
                        _ => 56,
                    }
                } else {
                    56
                }
            }
            SidebarMode::Overlay => 0,
            SidebarMode::Pushed => self.sidebar_width(),
        }
    }

    fn top_button_rects(&self) -> (RECT, RECT, RECT) {
        let x = self.top_button_x();
        (
            RECT {
                left: x,
                top: 13,
                right: x + 36,
                bottom: 49,
            },
            RECT {
                left: x + 44,
                top: 13,
                right: x + 80,
                bottom: 49,
            },
            RECT {
                left: x + 88,
                top: 13,
                right: x + 124,
                bottom: 49,
            },
        )
    }

    fn logo_rect(&self) -> RECT {
        RECT {
            left: 16,
            top: 14,
            right: 48,
            bottom: 46,
        }
    }

    fn new_tab_rect(&self) -> RECT {
        let right = self.sidebar_width().max(SIDEBAR_EXPANDED as i32) - 16;
        RECT {
            left: right - 36,
            top: 13,
            right,
            bottom: 49,
        }
    }

    fn settings_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        RECT {
            left: 16,
            top: rect.bottom - 52,
            right: 48,
            bottom: rect.bottom - 20,
        }
    }

    fn settings_menu_rect(&self) -> RECT {
        let settings = self.settings_rect();
        let bottom = settings.top - 8;
        RECT {
            left: 12,
            top: bottom - 108,
            right: 196,
            bottom,
        }
    }

    fn mode_row_rect(&self) -> RECT {
        let menu = self.settings_menu_rect();
        RECT {
            left: menu.left + 8,
            top: menu.top + 10,
            right: menu.right - 8,
            bottom: menu.top + 46,
        }
    }

    fn mode_options_rect(&self) -> RECT {
        let row = self.mode_row_rect();
        RECT {
            left: row.right + 8,
            top: row.top - 6,
            right: row.right + 132,
            bottom: row.top + 108,
        }
    }

    fn address_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let (_, _, reload) = self.top_button_rects();
        RECT {
            left: reload.right + 14,
            top: 13,
            right: (rect.right - 18).max(reload.right + 220),
            bottom: 49,
        }
    }

    fn new_tab_opacity(&self) -> f32 {
        ((self.sidebar_width - 118.0) / (SIDEBAR_EXPANDED - 118.0)).clamp(0.0, 1.0)
    }

    fn layout(&self) {
        let rect = client_rect(self.hwnd);
        let address = self.address_rect();
        unsafe {
            let flags = if self.animating_sidebar {
                WindowsAndMessaging::SWP_NOZORDER | WindowsAndMessaging::SWP_NOREDRAW
            } else {
                WindowsAndMessaging::SWP_NOZORDER
            };
            let _ = WindowsAndMessaging::SetWindowPos(
                self.address_hwnd,
                None,
                address.left + 36,
                address.top + 7,
                (address.right - address.left - 52).max(120),
                22,
                flags,
            );
        }

        let sidebar_width = self.sidebar_width();
        let bounds = match self.sidebar_mode {
            SidebarMode::Hidden => {
                if self.sidebar_target >= SIDEBAR_EXPANDED {
                    match self.sidebar_expand_mode {
                        SidebarMode::Overlay => RECT {
                            left: 0,
                            top: TOPBAR_HEIGHT,
                            right: rect.right,
                            bottom: rect.bottom,
                        },
                        SidebarMode::Pushed => RECT {
                            left: sidebar_width,
                            top: TOPBAR_HEIGHT,
                            right: rect.right,
                            bottom: rect.bottom,
                        },
                        _ => RECT {
                            left: HOVER_ZONE,
                            top: TOPBAR_HEIGHT,
                            right: rect.right,
                            bottom: rect.bottom,
                        },
                    }
                } else {
                    RECT {
                        left: HOVER_ZONE,
                        top: TOPBAR_HEIGHT,
                        right: rect.right,
                        bottom: rect.bottom,
                    }
                }
            }
            SidebarMode::Overlay => RECT {
                left: 0,
                top: TOPBAR_HEIGHT,
                right: rect.right,
                bottom: rect.bottom,
            },
            SidebarMode::Pushed => RECT {
                left: sidebar_width,
                top: TOPBAR_HEIGHT,
                right: rect.right,
                bottom: rect.bottom,
            },
        };
        for (i, tab) in self.tabs.iter().enumerate() {
            unsafe {
                let _ = tab.controller.SetBounds(bounds);
                let _ = tab.controller.SetIsVisible(i == self.active);
            }
        }
    }

    fn paint(&self, hdc: HDC) {
        let rect = client_rect(self.hwnd);
        let sidebar_width = self.sidebar_width();
        let is_overlay = self.sidebar_mode == SidebarMode::Overlay;
        unsafe {
            let _ = FillRect(hdc, &rect, self.brushes.black);

            let topbar = RECT {
                left: 0,
                top: 0,
                right: rect.right,
                bottom: TOPBAR_HEIGHT,
            };
            let _ = FillRect(hdc, &topbar, self.brushes.panel);
            fill_rect(
                hdc,
                RECT {
                    left: 0,
                    top: TOPBAR_HEIGHT - 1,
                    right: rect.right,
                    bottom: TOPBAR_HEIGHT,
                },
                0x202020,
            );

            if sidebar_width > 0 {
                let sidebar = RECT {
                    left: 0,
                    top: 0,
                    right: sidebar_width,
                    bottom: rect.bottom,
                };
                let _ = FillRect(hdc, &sidebar, self.brushes.panel);
                if !is_overlay {
                    fill_rect(
                        hdc,
                        RECT {
                            left: sidebar.right - 1,
                            top: 0,
                            right: sidebar.right,
                            bottom: rect.bottom,
                        },
                        COLOR_BORDER,
                    );
                }
            }

            draw_logo(
                hdc,
                self.logo_rect(),
                self.hover_target == Some(HoverTarget::Logo),
            );
            let new_tab_opacity = self.new_tab_opacity();
            if new_tab_opacity > 0.08 {
                draw_icon_button(
                    hdc,
                    self.new_tab_rect(),
                    IconKind::Plus,
                    self.hover_target == Some(HoverTarget::NewTab),
                    new_tab_opacity,
                    &self.fonts.icon,
                );
            }

            let (back, forward, reload) = self.top_button_rects();
            draw_icon_button(
                hdc,
                back,
                IconKind::Back,
                self.hover_target == Some(HoverTarget::Back),
                1.0,
                &self.fonts.icon,
            );
            draw_icon_button(
                hdc,
                forward,
                IconKind::Forward,
                self.hover_target == Some(HoverTarget::Forward),
                1.0,
                &self.fonts.icon,
            );
            draw_icon_button(
                hdc,
                reload,
                IconKind::Reload,
                self.hover_target == Some(HoverTarget::Reload),
                1.0,
                &self.fonts.icon,
            );

            let edit_rect = self.address_rect();
            fill_round_rect(hdc, edit_rect, 0x151515, 12);
            draw_outline(hdc, edit_rect, COLOR_BORDER, 12);
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xE774).as_str(),
                RECT {
                    left: edit_rect.left + 10,
                    top: edit_rect.top,
                    right: edit_rect.left + 34,
                    bottom: edit_rect.bottom,
                },
                COLOR_MUTED,
            );

            draw_settings_button(
                hdc,
                self.settings_rect(),
                self.hover_target == Some(HoverTarget::Settings),
                &self.fonts.icon,
            );

            if sidebar_width > 92 {
                for (index, tab) in self.tabs.iter().enumerate() {
                    self.paint_tab(hdc, index, tab);
                }
            }

            if self.settings_open {
                self.paint_settings_menu(hdc);
            }
        }
    }

    fn paint_settings_menu(&self, hdc: HDC) {
        unsafe {
            let menu = self.settings_menu_rect();
            fill_round_rect(hdc, menu, 0x151515, 12);
            draw_outline(hdc, menu, COLOR_BORDER, 12);

            let row = self.mode_row_rect();
            let row_hover = self.hover_target == Some(HoverTarget::ModeRow)
                || matches!(
                    self.hover_target,
                    Some(HoverTarget::ModeAuto | HoverTarget::ModeDark | HoverTarget::ModeLight)
                );
            if row_hover || self.mode_menu_open {
                fill_round_rect(hdc, row, COLOR_SURFACE_HOVER, 9);
            }
            draw_text(
                hdc,
                &self.fonts.small,
                "Mode",
                RECT {
                    left: row.left + 12,
                    top: row.top,
                    right: row.right - 62,
                    bottom: row.bottom,
                },
                COLOR_TEXT,
            );
            draw_text(
                hdc,
                &self.fonts.small,
                self.site_mode.label(),
                RECT {
                    left: row.right - 58,
                    top: row.top,
                    right: row.right - 24,
                    bottom: row.bottom,
                },
                COLOR_MUTED,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xE76C).as_str(),
                RECT {
                    left: row.right - 24,
                    top: row.top,
                    right: row.right - 6,
                    bottom: row.bottom,
                },
                COLOR_MUTED,
            );

            if self.mode_menu_open {
                let options = self.mode_options_rect();
                fill_round_rect(hdc, options, 0x151515, 12);
                draw_outline(hdc, options, COLOR_BORDER, 12);
                let modes = [
                    (SiteMode::Auto, HoverTarget::ModeAuto, "Auto"),
                    (SiteMode::Dark, HoverTarget::ModeDark, "Dark"),
                    (SiteMode::Light, HoverTarget::ModeLight, "Light"),
                ];
                for (index, (mode, hover, label)) in modes.iter().enumerate() {
                    let top = options.top + 8 + index as i32 * 34;
                    let item = RECT {
                        left: options.left + 8,
                        top,
                        right: options.right - 8,
                        bottom: top + 30,
                    };
                    if self.hover_target == Some(*hover) {
                        fill_round_rect(hdc, item, COLOR_SURFACE_HOVER, 8);
                    }
                    if self.site_mode == *mode {
                        fill_round_rect(
                            hdc,
                            RECT {
                                left: item.left + 8,
                                top: item.top + 11,
                                right: item.left + 16,
                                bottom: item.top + 19,
                            },
                            COLOR_ACCENT,
                            4,
                        );
                    }
                    draw_text(
                        hdc,
                        &self.fonts.small,
                        label,
                        RECT {
                            left: item.left + 24,
                            top: item.top,
                            right: item.right - 8,
                            bottom: item.bottom,
                        },
                        COLOR_TEXT,
                    );
                }
            }
        }
    }

    fn paint_tab(&self, hdc: HDC, index: usize, tab: &Tab) {
        let top = TAB_TOP + index as i32 * TAB_HEIGHT;
        let right = self.sidebar_width() - 12;
        let item = RECT {
            left: 12,
            top,
            right,
            bottom: top + TAB_HEIGHT - 8,
        };
        unsafe {
            if self.hover_tab == Some(index) {
                fill_round_rect(hdc, item, 0x151515, 10);
            }
            let favicon = RECT {
                left: item.left + 12,
                top: item.top + 11,
                right: item.left + 30,
                bottom: item.top + 29,
            };
            fill_round_rect(
                hdc,
                favicon,
                if index == self.active {
                    COLOR_ACCENT
                } else {
                    0x2a2a2a
                },
                6,
            );
            draw_text(
                hdc,
                &self.fonts.body,
                &tab.title,
                RECT {
                    left: item.left + 40,
                    top: item.top,
                    right: item.right - 36,
                    bottom: item.bottom,
                },
                if index == self.active {
                    COLOR_TEXT
                } else {
                    COLOR_MUTED
                },
            );
            if self.hover_tab == Some(index) {
                let close_color = if self.hover_close == Some(index) {
                    COLOR_TEXT
                } else {
                    COLOR_MUTED
                };
                draw_icon_glyph(
                    hdc,
                    &self.fonts.icon,
                    glyph(0xE711).as_str(),
                    RECT {
                        left: item.right - 30,
                        top: item.top,
                        right: item.right - 8,
                        bottom: item.bottom,
                    },
                    close_color,
                );
            }
        }
    }

    fn handle_click(&mut self, x: i32, y: i32) {
        if self.sidebar_mode == SidebarMode::Overlay
            && self.sidebar_width > SIDEBAR_EXPANDED * 0.5
            && (x as f32) >= self.sidebar_width
        {
            self.set_sidebar_mode(SidebarMode::Hidden);
            return;
        }

        if self.settings_open {
            if self.mode_menu_open {
                let options = self.mode_options_rect();
                if point_in_rect(x, y, options) {
                    let local_y = y - options.top - 8;
                    if local_y >= 0 {
                        match local_y / 34 {
                            0 => self.set_site_mode(SiteMode::Auto),
                            1 => self.set_site_mode(SiteMode::Dark),
                            2 => self.set_site_mode(SiteMode::Light),
                            _ => {}
                        }
                    }
                    return;
                }
            }

            if point_in_rect(x, y, self.mode_row_rect()) {
                self.mode_menu_open = !self.mode_menu_open;
                self.refresh();
                return;
            }

            if !point_in_rect(x, y, self.settings_menu_rect())
                && !point_in_rect(x, y, self.settings_rect())
            {
                self.settings_open = false;
                self.mode_menu_open = false;
                self.refresh();
            }
        }

        if point_in_rect(x, y, self.logo_rect()) {
            self.toggle_sidebar();
            return;
        }

        if self.new_tab_opacity() > 0.6 && point_in_rect(x, y, self.new_tab_rect()) {
            let _ = self.create_tab(DEFAULT_URL);
            return;
        }

        let (back, forward, reload) = self.top_button_rects();
        if point_in_rect(x, y, back) {
            self.go_back();
            return;
        }
        if point_in_rect(x, y, forward) {
            self.go_forward();
            return;
        }
        if point_in_rect(x, y, reload) {
            self.reload();
            return;
        }

        if point_in_rect(x, y, self.settings_rect()) {
            self.settings_open = !self.settings_open;
            self.mode_menu_open = false;
            self.refresh();
            return;
        }

        if self.sidebar_width() > 92 && (x as f32) < self.sidebar_width && y >= TAB_TOP {
            let index = ((y - TAB_TOP) / TAB_HEIGHT) as usize;
            if index < self.tabs.len() {
                let close_left = self.sidebar_width() - 42;
                if x >= close_left {
                    self.close_tab(index);
                } else {
                    self.switch_to(index);
                }
            }
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        let old_close = self.hover_close;
        let old_tab = self.hover_tab;
        let old_target = self.hover_target;
        let old_mode_menu = self.mode_menu_open;
        let old_hovering = self.hovering_sidebar;
        self.hover_close = None;
        self.hover_tab = None;
        self.hover_target = None;

        if x < HOVER_ZONE && self.sidebar_mode == SidebarMode::Hidden && !self.animating_sidebar {
            self.sidebar_expand_mode = SidebarMode::Overlay;
            self.set_sidebar_mode(SidebarMode::Overlay);
        }

        if (x as f32) < self.sidebar_width + 4.0 && self.sidebar_width > 0.5 {
            self.hovering_sidebar = true;
        } else {
            self.hovering_sidebar = false;
        }

        if point_in_rect(x, y, self.logo_rect()) {
            self.hover_target = Some(HoverTarget::Logo);
        } else if self.new_tab_opacity() > 0.6 && point_in_rect(x, y, self.new_tab_rect()) {
            self.hover_target = Some(HoverTarget::NewTab);
        } else {
            let (back, forward, reload) = self.top_button_rects();
            if point_in_rect(x, y, back) {
                self.hover_target = Some(HoverTarget::Back);
            } else if point_in_rect(x, y, forward) {
                self.hover_target = Some(HoverTarget::Forward);
            } else if point_in_rect(x, y, reload) {
                self.hover_target = Some(HoverTarget::Reload);
            } else if point_in_rect(x, y, self.settings_rect()) {
                self.hover_target = Some(HoverTarget::Settings);
            } else if self.settings_open && point_in_rect(x, y, self.mode_row_rect()) {
                self.hover_target = Some(HoverTarget::ModeRow);
                self.mode_menu_open = true;
            } else if self.settings_open
                && self.mode_menu_open
                && point_in_rect(x, y, self.mode_options_rect())
            {
                let options = self.mode_options_rect();
                let local_y = y - options.top - 8;
                if local_y >= 0 {
                    self.hover_target = match local_y / 34 {
                        0 => Some(HoverTarget::ModeAuto),
                        1 => Some(HoverTarget::ModeDark),
                        2 => Some(HoverTarget::ModeLight),
                        _ => None,
                    };
                }
            }
        }

        if self.sidebar_width() > 92 && (x as f32) < self.sidebar_width && y >= TAB_TOP {
            let index = ((y - TAB_TOP) / TAB_HEIGHT) as usize;
            if index < self.tabs.len() {
                self.hover_tab = Some(index);
                if x >= self.sidebar_width() - 42 {
                    self.hover_close = Some(index);
                }
            }
        }
        if old_close != self.hover_close
            || old_tab != self.hover_tab
            || old_target != self.hover_target
            || old_mode_menu != self.mode_menu_open
            || old_hovering != self.hovering_sidebar
        {
            self.refresh();
        }
    }

    fn toggle_sidebar(&mut self) {
        match self.sidebar_mode {
            SidebarMode::Hidden => {
                self.sidebar_expand_mode = SidebarMode::Pushed;
                self.set_sidebar_mode(SidebarMode::Pushed);
            }
            SidebarMode::Pushed => self.set_sidebar_mode(SidebarMode::Hidden),
            SidebarMode::Overlay => self.set_sidebar_mode(SidebarMode::Hidden),
        }
    }

    fn set_sidebar_mode(&mut self, mode: SidebarMode) {
        self.sidebar_target = match mode {
            SidebarMode::Hidden => SIDEBAR_HIDDEN,
            SidebarMode::Overlay | SidebarMode::Pushed => SIDEBAR_EXPANDED,
        };
        self.animating_sidebar = true;
        unsafe {
            let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
            let _ = WindowsAndMessaging::SetTimer(Some(self.hwnd), SIDEBAR_TIMER_ID, 15, None);
        }
    }

    fn tick_sidebar_animation(&mut self) {
        let distance = self.sidebar_target - self.sidebar_width;
        if distance.abs() < 0.75 {
            self.sidebar_width = self.sidebar_target;
            self.animating_sidebar = false;
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), SIDEBAR_TIMER_ID);
            }
            if self.sidebar_width < 0.5 {
                self.sidebar_mode = SidebarMode::Hidden;
            } else if self.sidebar_target >= SIDEBAR_EXPANDED {
                self.sidebar_mode = self.sidebar_expand_mode;
                if self.sidebar_mode == SidebarMode::Overlay {
                    unsafe {
                        let _ = WindowsAndMessaging::SetTimer(
                            Some(self.hwnd),
                            HOVER_LEAVE_TIMER_ID,
                            100,
                            None,
                        );
                    }
                }
            }
        } else {
            self.sidebar_width += distance * 0.22;
        }
        self.layout();
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = Gdi::UpdateWindow(self.hwnd);
        }
    }

    fn check_hover_leave(&mut self) {
        if self.sidebar_mode != SidebarMode::Overlay {
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
            }
            return;
        }
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                if ScreenToClient(self.hwnd, &mut pt).as_bool() {
                    let sidebar_w = self.sidebar_width() as i32;
                    if pt.x > sidebar_w + 20 || pt.x < 0 || pt.y < 0 {
                        let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
                        self.sidebar_expand_mode = SidebarMode::Hidden;
                        self.set_sidebar_mode(SidebarMode::Hidden);
                    }
                }
            }
        }
    }

    fn set_site_mode(&mut self, mode: SiteMode) {
        self.site_mode = mode;
        self.settings_open = false;
        self.mode_menu_open = false;
        for tab in &self.tabs {
            apply_site_mode_to_webview(&tab.webview, self.site_mode);
            unsafe {
                let _ = tab.webview.Reload();
            }
        }
        self.refresh();
    }

    fn toggle_fullscreen(&mut self) {
        unsafe {
            if !self.fullscreen {
                let _ = WindowsAndMessaging::GetWindowRect(self.hwnd, &mut self.saved_rect);
                self.saved_style = WindowsAndMessaging::GetWindowLongPtrW(self.hwnd, GWL_STYLE);

                let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                let mut monitor_info = MONITORINFO {
                    cbSize: mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };

                if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                    let _ = SetWindowLong(self.hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                    let bounds = monitor_info.rcMonitor;
                    let _ = WindowsAndMessaging::SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOP),
                        bounds.left,
                        bounds.top,
                        bounds.right - bounds.left,
                        bounds.bottom - bounds.top,
                        WindowsAndMessaging::SWP_FRAMECHANGED
                            | WindowsAndMessaging::SWP_NOOWNERZORDER,
                    );
                    self.fullscreen = true;
                }
            } else {
                let _ = SetWindowLong(self.hwnd, GWL_STYLE, self.saved_style);
                let rect = self.saved_rect;
                let _ = WindowsAndMessaging::SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOP),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    WindowsAndMessaging::SWP_FRAMECHANGED | WindowsAndMessaging::SWP_NOOWNERZORDER,
                );
                self.fullscreen = false;
            }
        }
        self.layout();
        self.refresh();
    }

    fn refresh(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

fn main() -> AppResult<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    set_process_dpi_awareness();
    register_window_class()?;
    let hwnd = create_main_window()?;
    let environment = create_environment()?;

    let app = Box::new(App::new(hwnd, environment)?);
    unsafe {
        SetWindowLong(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        let _ = WindowsAndMessaging::ShowWindow(hwnd, WindowsAndMessaging::SW_SHOW);
        let _ = Gdi::UpdateWindow(hwnd);
    }

    message_loop()
}

fn create_environment() -> AppResult<ICoreWebView2Environment> {
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|handler| unsafe {
            let user_data = CoTaskMemPWSTR::from(".aster-profile");
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                *user_data.as_ref().as_pcwstr(),
                None,
                &handler,
            )
            .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, environment| {
            error_code?;
            tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .expect("send WebView2 environment over startup channel");
            Ok(())
        }),
    )?;
    Ok(rx.recv()??)
}

fn create_webview_controller(
    environment: &ICoreWebView2Environment,
    hwnd: HWND,
) -> AppResult<ICoreWebView2Controller> {
    let (tx, rx) = mpsc::channel();
    let environment = environment.clone();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            environment
                .CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, controller| {
            error_code?;
            tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .expect("send WebView2 controller over startup channel");
            Ok(())
        }),
    )?;
    Ok(rx.recv()??)
}

fn configure_webview(webview: &ICoreWebView2) -> AppResult<()> {
    unsafe {
        let settings = webview.Settings()?;
        settings.SetAreDefaultScriptDialogsEnabled(true)?;
        settings.SetAreDevToolsEnabled(true)?;
        settings.SetIsStatusBarEnabled(false)?;
        let settings3: ICoreWebView2Settings3 = settings.cast()?;
        settings3.SetAreBrowserAcceleratorKeysEnabled(true)?;
    }
    Ok(())
}

fn apply_site_mode_to_webview(webview: &ICoreWebView2, mode: SiteMode) {
    unsafe {
        if let Ok(webview13) = webview.cast::<ICoreWebView2_13>() {
            if let Ok(profile) = webview13.Profile() {
                let _ = profile.SetPreferredColorScheme(mode.scheme());
            }
        }
    }
}

fn register_window_class() -> AppResult<()> {
    unsafe {
        let hinstance = HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0);
        let cursor = WindowsAndMessaging::LoadCursorW(None, IDC_ARROW)?;
        let wc = WNDCLASSW {
            hCursor: cursor,
            hInstance: hinstance,
            lpszClassName: CLASS_NAME,
            lpfnWndProc: Some(window_proc),
            style: WindowsAndMessaging::CS_HREDRAW | WindowsAndMessaging::CS_VREDRAW,
            ..Default::default()
        };
        if WindowsAndMessaging::RegisterClassW(&wc) == 0 {
            return Err(windows::core::Error::from_win32().into());
        }
    }
    Ok(())
}

fn create_main_window() -> AppResult<HWND> {
    unsafe {
        let hinstance = HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0);
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            CLASS_NAME,
            APP_NAME,
            WS_OVERLAPPEDWINDOW | WS_CLIPSIBLINGS,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            820,
            None,
            None,
            Some(hinstance),
            None,
        )?;
        let _ = WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(0)),
        );
        let _ = WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(0)),
        );
        enable_dark_titlebar(hwnd);
        Ok(hwnd)
    }
}

fn enable_dark_titlebar(hwnd: HWND) {
    unsafe {
        let enabled = 1i32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const _ as *const _,
            mem::size_of_val(&enabled) as u32,
        );
        let caption = COLOR_PANEL;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption as *const _ as *const _,
            mem::size_of_val(&caption) as u32,
        );
        let text = COLOR_TEXT;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &text as *const _ as *const _,
            mem::size_of_val(&text) as u32,
        );
    }
}

fn create_address_bar(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
            SIDEBAR_EXPANDED as i32 + 168,
            20,
            680,
            22,
            Some(parent),
            Some(HMENU(ADDRESS_ID as usize as *mut _)),
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        let _ = WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_SETMARGINS,
            Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
            Some(LPARAM((2 | (2 << 16)) as isize)),
        );
        OLD_ADDRESS_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            address_bar_proc as *const () as isize,
        ));
        Ok(hwnd)
    }
}

unsafe extern "system" fn address_bar_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN && w_param.0 as u32 == VK_RETURN.0 as u32 {
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| app.navigate_active_from_address());
        }
        return LRESULT(0);
    }
    if msg == WM_CHAR && w_param.0 as u32 == VK_RETURN.0 as u32 {
        return LRESULT(0);
    }
    WindowsAndMessaging::CallWindowProcW(OLD_ADDRESS_PROC, hwnd, msg, w_param, l_param)
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let _ = l_param.0 as *const CREATESTRUCTW;
            LRESULT(1)
        }
        WM_CREATE => LRESULT(0),
        WM_SIZE => {
            with_app(hwnd, |app| app.layout());
            LRESULT(0)
        }
        WM_PAINT => {
            unsafe {
                let mut ps = mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let rect = client_rect(hwnd);
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                let mem_dc = CreateCompatibleDC(Some(hdc));
                let bitmap = CreateCompatibleBitmap(hdc, width, height);
                let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
                with_app(hwnd, |app| app.paint(mem_dc));
                let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);
                let _ = SelectObject(mem_dc, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(mem_dc);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| app.handle_click(x, y));
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| app.handle_mouse_move(x, y));
            LRESULT(0)
        }
        WM_TIMER => {
            if w_param.0 == SIDEBAR_TIMER_ID {
                with_app(hwnd, |app| app.tick_sidebar_animation());
                return LRESULT(0);
            }
            if w_param.0 == HOVER_LEAVE_TIMER_ID {
                with_app(hwnd, |app| app.check_hover_leave());
                return LRESULT(0);
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_COMMAND => {
            let id = loword(w_param.0 as u32) as i32;
            let code = hiword(w_param.0 as u32) as u16;
            if id == ADDRESS_ID && code == 0 {
                with_app(hwnd, |app| app.navigate_active_from_address());
                return LRESULT(0);
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_CHAR => {
            if w_param.0 as u32 == VK_RETURN.0 as u32 {
                with_app(hwnd, |app| app.navigate_active_from_address());
                return LRESULT(0);
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_KEYDOWN => {
            handle_keydown(hwnd, w_param);
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_SETFOCUS => {
            with_app(hwnd, |app| {
                if let Some(tab) = app.tabs.get(app.active) {
                    unsafe {
                        let _ = tab
                            .controller
                            .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
                    }
                }
            });
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => unsafe {
            let hdc = HDC(w_param.0 as *mut _);
            let _ = SetTextColor(hdc, COLORREF(COLOR_TEXT));
            let _ = SetBkMode(hdc, TRANSPARENT);
            let brush = with_app_return(hwnd, |app| app.brushes.edit)
                .unwrap_or_else(|| solid_brush(0x151515));
            LRESULT(brush.0 as isize)
        },
        WM_SETCURSOR => unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) },
        WM_ERASEBKGND => LRESULT(1),
        WM_CLOSE => {
            unsafe {
                let _ = WindowsAndMessaging::DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                if let Some(app) = take_app(hwnd) {
                    drop(app);
                }
                WindowsAndMessaging::PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) },
    }
}

fn handle_keydown(hwnd: HWND, w_param: WPARAM) {
    let key = w_param.0 as u32;
    unsafe {
        let ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
        let alt = (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
        with_app(hwnd, |app| match key {
            0x4C if ctrl => {
                let _ = SetFocus(Some(app.address_hwnd));
                let _ = WindowsAndMessaging::SendMessageW(
                    app.address_hwnd,
                    EM_SETSEL,
                    Some(WPARAM(0)),
                    Some(LPARAM(-1)),
                );
            }
            0x54 if ctrl => {
                let _ = app.create_tab(DEFAULT_URL);
            }
            0x53 if ctrl => app.toggle_sidebar(),
            0x57 if ctrl => app.close_tab(app.active),
            0x25 if alt => app.go_back(),
            0x27 if alt => app.go_forward(),
            code if code == VK_F5.0 as u32 => app.reload(),
            code if code == VK_F11.0 as u32 => app.toggle_fullscreen(),
            _ => {}
        });
    }
}

fn is_aster_shortcut(key: u32) -> bool {
    unsafe {
        let ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
        let alt = (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
        matches!(key, 0x4C | 0x53 | 0x54 | 0x57 if ctrl)
            || matches!(key, 0x25 | 0x27 if alt)
            || key == VK_F5.0 as u32
            || key == VK_F11.0 as u32
    }
}

fn message_loop() -> AppResult<()> {
    let mut msg = MSG::default();
    loop {
        unsafe {
            match WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0).0 {
                -1 => return Err(windows::core::Error::from_win32().into()),
                0 => return Ok(()),
                _ => {
                    if msg.message != WM_APP {
                        let _ = WindowsAndMessaging::TranslateMessage(&msg);
                        WindowsAndMessaging::DispatchMessageW(&msg);
                    }
                }
            }
        }
    }
}

fn create_font(size: i32, weight: i32) -> AppResult<HFONT> {
    create_font_with_face(size, weight, w!("Segoe UI"))
}

fn create_font_with_face(size: i32, weight: i32, face: PCWSTR) -> AppResult<HFONT> {
    unsafe {
        Ok(CreateFontW(
            -size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            Gdi::DEFAULT_CHARSET,
            Gdi::OUT_DEFAULT_PRECIS,
            Gdi::CLIP_DEFAULT_PRECIS,
            Gdi::CLEARTYPE_QUALITY,
            (Gdi::DEFAULT_PITCH.0 | Gdi::FF_SWISS.0) as u32,
            face,
        ))
    }
}

#[derive(Clone, Copy)]
enum IconKind {
    Plus,
    Back,
    Forward,
    Reload,
}

fn draw_icon_button(
    hdc: HDC,
    rect: RECT,
    icon: IconKind,
    hovered: bool,
    opacity: f32,
    icon_font: &HFONT,
) {
    unsafe {
        let fill = mix_color(
            COLOR_PANEL,
            if hovered {
                COLOR_SURFACE_HOVER
            } else {
                COLOR_SURFACE
            },
            opacity,
        );
        let border = mix_color(
            COLOR_PANEL,
            if hovered { 0x464646 } else { COLOR_BORDER },
            opacity,
        );
        let icon_color = mix_color(COLOR_PANEL, COLOR_TEXT, opacity);
        fill_round_rect(hdc, rect, fill, 10);
        draw_outline(hdc, rect, border, 10);
        draw_icon_glyph(hdc, icon_font, icon.glyph(), rect, icon_color);
    }
}

fn draw_logo(hdc: HDC, rect: RECT, hovered: bool) {
    unsafe {
        let color = if hovered { 0xff877f } else { COLOR_ACCENT };
        draw_aster_mark(hdc, rect, color);
    }
}

fn draw_settings_button(hdc: HDC, rect: RECT, hovered: bool, icon_font: &HFONT) {
    unsafe {
        if hovered {
            fill_round_rect(hdc, rect, COLOR_SURFACE_HOVER, 10);
        }
        draw_icon_glyph(hdc, icon_font, glyph(0xE713).as_str(), rect, COLOR_MUTED);
    }
}

impl IconKind {
    fn glyph(self) -> &'static str {
        match self {
            Self::Plus => "\u{E710}",
            Self::Back => "\u{E72B}",
            Self::Forward => "\u{E72A}",
            Self::Reload => "\u{E72C}",
        }
    }
}

unsafe fn draw_aster_mark(hdc: HDC, rect: RECT, color: u32) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    let radius = ((rect.right - rect.left).min(rect.bottom - rect.top) / 2) - 7;
    with_pen(hdc, color, 5, || {
        let _ = MoveToEx(hdc, cx, cy - radius, None);
        let _ = LineTo(hdc, cx, cy + radius);
        let dx = radius * 9 / 10;
        let dy = radius / 2;
        let _ = MoveToEx(hdc, cx - dx, cy - dy, None);
        let _ = LineTo(hdc, cx + dx, cy + dy);
        let _ = MoveToEx(hdc, cx + dx, cy - dy, None);
        let _ = LineTo(hdc, cx - dx, cy + dy);
    });
}

unsafe fn with_pen<F>(hdc: HDC, color: u32, width: i32, f: F)
where
    F: FnOnce(),
{
    let pen = CreatePen(Gdi::PS_SOLID, width, COLORREF(color));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    f();
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn fill_round_rect(hdc: HDC, rect: RECT, color: u32, radius: i32) {
    let brush = solid_brush(color);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let _ = RoundRect(
        hdc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn fill_rect(hdc: HDC, rect: RECT, color: u32) {
    let brush = solid_brush(color);
    let _ = FillRect(hdc, &rect, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn draw_outline(hdc: HDC, rect: RECT, color: u32, radius: i32) {
    let pen = CreatePen(Gdi::PS_SOLID, 1, COLORREF(color));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let _ = RoundRect(
        hdc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_text(hdc: HDC, font: &HFONT, text: &str, mut rect: RECT, color: u32) {
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, COLORREF(color));
    let mut wide = to_wide(text);
    let text_len = wide.len().saturating_sub(1);
    let _ = DrawTextW(
        hdc,
        &mut wide[..text_len],
        &mut rect,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    let _ = SelectObject(hdc, old_font);
}

unsafe fn draw_icon_glyph(hdc: HDC, font: &HFONT, text: &str, mut rect: RECT, color: u32) {
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, COLORREF(color));
    let mut wide = to_wide(text);
    let text_len = wide.len().saturating_sub(1);
    let _ = DrawTextW(
        hdc,
        &mut wide[..text_len],
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    let _ = SelectObject(hdc, old_font);
}

fn glyph(codepoint: u32) -> String {
    char::from_u32(codepoint).unwrap_or(' ').to_string()
}

fn mix_color(from: u32, to: u32, amount: f32) -> u32 {
    let t = amount.clamp(0.0, 1.0);
    let fr = (from & 0xff) as f32;
    let fg = ((from >> 8) & 0xff) as f32;
    let fb = ((from >> 16) & 0xff) as f32;
    let tr = (to & 0xff) as f32;
    let tg = ((to >> 8) & 0xff) as f32;
    let tb = ((to >> 16) & 0xff) as f32;
    let r = (fr + (tr - fr) * t).round() as u32;
    let g = (fg + (tg - fg) * t).round() as u32;
    let b = (fb + (tb - fb) * t).round() as u32;
    r | (g << 8) | (b << 16)
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    unsafe {
        let _ = WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
    }
    rect
}

fn set_process_dpi_awareness() {
    unsafe {
        let _ = HiDpi::SetProcessDpiAwareness(HiDpi::PROCESS_PER_MONITOR_DPI_AWARE);
    }
}

fn set_window_text(hwnd: HWND, text: &str) {
    let wide = to_wide(text);
    unsafe {
        let _ = WindowsAndMessaging::SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
    }
}

fn get_window_text(hwnd: HWND) -> String {
    unsafe {
        let len = WindowsAndMessaging::GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len as usize + 1];
        let copied = WindowsAndMessaging::GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..copied as usize])
    }
}

fn normalize_address(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        return DEFAULT_URL.to_string();
    }
    if value.contains("://") || value.starts_with("about:") {
        value.to_string()
    } else if value.contains('.') && !value.contains(' ') {
        format!("https://{value}")
    } else {
        format!(
            "https://www.google.com/search?q={}",
            value.replace(' ', "+")
        )
    }
}

fn label_for_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("New Tab")
        .to_string()
}

fn solid_brush(color: u32) -> HBRUSH {
    unsafe { CreateSolidBrush(COLORREF(color)) }
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn loword(value: u32) -> u16 {
    (value & 0xffff) as u16
}

fn hiword(value: u32) -> u16 {
    ((value >> 16) & 0xffff) as u16
}

fn point_in_rect(x: i32, y: i32, rect: RECT) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn with_app<F>(hwnd: HWND, f: F)
where
    F: FnOnce(&mut App),
{
    unsafe {
        let ptr = WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !ptr.is_null() {
            f(&mut *ptr);
        }
    }
}

fn with_app_return<T, F>(hwnd: HWND, f: F) -> Option<T>
where
    F: FnOnce(&mut App) -> T,
{
    unsafe {
        let ptr = WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if ptr.is_null() {
            None
        } else {
            Some(f(&mut *ptr))
        }
    }
}

unsafe fn take_app(hwnd: HWND) -> Option<Box<App>> {
    let ptr = WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        None
    } else {
        let _ = SetWindowLong(hwnd, GWLP_USERDATA, 0);
        Some(Box::from_raw(ptr))
    }
}

#[allow(non_snake_case)]
#[cfg(target_pointer_width = "32")]
unsafe fn SetWindowLong(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    WindowsAndMessaging::SetWindowLongW(window, index, value as _) as _
}

#[allow(non_snake_case)]
#[cfg(target_pointer_width = "64")]
unsafe fn SetWindowLong(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    WindowsAndMessaging::SetWindowLongPtrW(window, index, value)
}
