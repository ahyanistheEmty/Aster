#![windows_subsystem = "windows"]

use std::{mem, sync::mpsc};

use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::*,
    Win32::{
        Foundation::{COLORREF, E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            self, BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW,
            EndPaint, FillRect, GetStockObject, InvalidateRect, RoundRect, SelectObject,
            SetBkMode, SetTextColor, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
            HBRUSH, HDC, HFONT, HGDIOBJ, NULL_BRUSH, NULL_PEN, TRANSPARENT,
        },
        System::{Com::*, LibraryLoader},
        UI::{
            Controls::{EM_SETMARGINS, EM_SETSEL},
            HiDpi,
            Input::KeyboardAndMouse::{
                GetKeyState, SetFocus, VK_CONTROL, VK_F5, VK_MENU, VK_RETURN,
            },
            WindowsAndMessaging::{
                self, CREATESTRUCTW, CW_USEDEFAULT, GWLP_USERDATA, HMENU, IDC_ARROW, MSG,
                EC_LEFTMARGIN, EC_RIGHTMARGIN, GWLP_WNDPROC, WNDPROC,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP, WM_CHAR, WM_CLOSE,
                WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC,
                WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT,
                WM_SETCURSOR, WM_SETFOCUS, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD,
                WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
};

const APP_NAME: PCWSTR = w!("Aster");
const CLASS_NAME: PCWSTR = w!("AsterWindow");
const ADDRESS_ID: i32 = 1001;
const SIDEBAR_WIDTH: i32 = 232;
const TOPBAR_HEIGHT: i32 = 58;
const TAB_HEIGHT: i32 = 42;
const TAB_TOP: i32 = 78;

const COLOR_BLACK: u32 = 0x000000;
const COLOR_PANEL: u32 = 0x090909;
const COLOR_PANEL_2: u32 = 0x111111;
const COLOR_ACTIVE: u32 = 0x1c1c1c;
const COLOR_BORDER: u32 = 0x2b2b2b;
const COLOR_TEXT: u32 = 0xf5f5f5;
const COLOR_MUTED: u32 = 0xa1a1a1;

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

impl Drop for Tab {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

struct UiFonts {
    title: HFONT,
    body: HFONT,
    small: HFONT,
}

impl Drop for UiFonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.title.0));
            let _ = DeleteObject(HGDIOBJ(self.body.0));
            let _ = DeleteObject(HGDIOBJ(self.small.0));
        }
    }
}

struct UiBrushes {
    black: HBRUSH,
    panel: HBRUSH,
    panel_2: HBRUSH,
    active: HBRUSH,
    edit: HBRUSH,
}

impl Drop for UiBrushes {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.black.0));
            let _ = DeleteObject(HGDIOBJ(self.panel.0));
            let _ = DeleteObject(HGDIOBJ(self.panel_2.0));
            let _ = DeleteObject(HGDIOBJ(self.active.0));
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
}

impl App {
    fn new(hwnd: HWND, environment: ICoreWebView2Environment) -> AppResult<Self> {
        let fonts = UiFonts {
            title: create_font(20, 700)?,
            body: create_font(15, 500)?,
            small: create_font(13, 500)?,
        };
        let brushes = UiBrushes {
            black: solid_brush(COLOR_BLACK),
            panel: solid_brush(COLOR_PANEL),
            panel_2: solid_brush(COLOR_PANEL_2),
            active: solid_brush(COLOR_ACTIVE),
            edit: solid_brush(0x151515),
        };

        let address_hwnd = create_address_bar(hwnd)?;
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
        };
        app.create_tab("https://vercel.com")?;
        Ok(app)
    }

    fn create_tab(&mut self, url: &str) -> AppResult<()> {
        let controller = create_webview_controller(&self.environment, self.hwnd)?;
        let webview = unsafe { controller.CoreWebView2()? };
        configure_webview(&webview)?;

        let id = self.next_id;
        self.next_id += 1;
        let index = self.tabs.len();
        self.attach_events(index, id, &webview)?;

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

    fn attach_events(&self, index_hint: usize, tab_id: usize, webview: &ICoreWebView2) -> AppResult<()> {
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
            tab.url = if url == "about:blank" { String::new() } else { url };
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
                let _ = tab.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
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
            let _ = self.create_tab("https://vercel.com");
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

    fn layout(&self) {
        let rect = client_rect(self.hwnd);
        unsafe {
            let _ = WindowsAndMessaging::SetWindowPos(
                self.address_hwnd,
                None,
                SIDEBAR_WIDTH + 132,
                14,
                (rect.right - SIDEBAR_WIDTH - 222).max(180),
                32,
                WindowsAndMessaging::SWP_NOZORDER,
            );
        }

        let bounds = RECT {
            left: SIDEBAR_WIDTH,
            top: TOPBAR_HEIGHT,
            right: rect.right,
            bottom: rect.bottom,
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
        unsafe {
            let _ = FillRect(hdc, &rect, self.brushes.black);

            let sidebar = RECT {
                left: 0,
                top: 0,
                right: SIDEBAR_WIDTH,
                bottom: rect.bottom,
            };
            let _ = FillRect(hdc, &sidebar, self.brushes.panel);

            let topbar = RECT {
                left: SIDEBAR_WIDTH,
                top: 0,
                right: rect.right,
                bottom: TOPBAR_HEIGHT,
            };
            let _ = FillRect(hdc, &topbar, self.brushes.panel);

            draw_text(
                hdc,
                &self.fonts.title,
                "Aster",
                RECT {
                    left: 20,
                    top: 18,
                    right: 120,
                    bottom: 48,
                },
                COLOR_TEXT,
            );
            draw_button(hdc, RECT { left: 172, top: 14, right: 212, bottom: 46 }, "+", &self.fonts.body);

            draw_button(hdc, RECT { left: SIDEBAR_WIDTH + 18, top: 14, right: SIDEBAR_WIDTH + 50, bottom: 46 }, "<", &self.fonts.body);
            draw_button(hdc, RECT { left: SIDEBAR_WIDTH + 58, top: 14, right: SIDEBAR_WIDTH + 90, bottom: 46 }, ">", &self.fonts.body);
            draw_button(hdc, RECT { left: SIDEBAR_WIDTH + 98, top: 14, right: SIDEBAR_WIDTH + 130, bottom: 46 }, "R", &self.fonts.body);

            let edit_rect = RECT {
                left: SIDEBAR_WIDTH + 132,
                top: 14,
                right: (rect.right - 90).max(SIDEBAR_WIDTH + 330),
                bottom: 46,
            };
            draw_outline(hdc, edit_rect, COLOR_BORDER, 8);

            for (index, tab) in self.tabs.iter().enumerate() {
                self.paint_tab(hdc, index, tab);
            }
        }
    }

    fn paint_tab(&self, hdc: HDC, index: usize, tab: &Tab) {
        let top = TAB_TOP + index as i32 * TAB_HEIGHT;
        let item = RECT {
            left: 12,
            top,
            right: SIDEBAR_WIDTH - 12,
            bottom: top + TAB_HEIGHT - 6,
        };
        unsafe {
            let brush = if index == self.active {
                self.brushes.active
            } else if self.hover_tab == Some(index) {
                self.brushes.panel_2
            } else {
                self.brushes.panel
            };
            let _ = FillRect(hdc, &item, brush);
            draw_outline(hdc, item, if index == self.active { COLOR_BORDER } else { COLOR_PANEL_2 }, 8);
            draw_text(
                hdc,
                &self.fonts.body,
                &tab.title,
                RECT {
                    left: item.left + 14,
                    top: item.top,
                    right: item.right - 34,
                    bottom: item.bottom,
                },
                if index == self.active { COLOR_TEXT } else { COLOR_MUTED },
            );
            let close_color = if self.hover_close == Some(index) { COLOR_TEXT } else { COLOR_MUTED };
            draw_text(
                hdc,
                &self.fonts.small,
                "x",
                RECT {
                    left: item.right - 26,
                    top: item.top,
                    right: item.right - 8,
                    bottom: item.bottom,
                },
                close_color,
            );
        }
    }

    fn handle_click(&mut self, x: i32, y: i32) {
        if y >= 14 && y <= 46 {
            if x >= 172 && x <= 212 {
                let _ = self.create_tab("https://vercel.com");
                return;
            }
            if x >= SIDEBAR_WIDTH + 18 && x <= SIDEBAR_WIDTH + 50 {
                self.go_back();
                return;
            }
            if x >= SIDEBAR_WIDTH + 58 && x <= SIDEBAR_WIDTH + 90 {
                self.go_forward();
                return;
            }
            if x >= SIDEBAR_WIDTH + 98 && x <= SIDEBAR_WIDTH + 130 {
                self.reload();
                return;
            }
        }

        if x < SIDEBAR_WIDTH && y >= TAB_TOP {
            let index = ((y - TAB_TOP) / TAB_HEIGHT) as usize;
            if index < self.tabs.len() {
                let close_left = SIDEBAR_WIDTH - 38;
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
        self.hover_close = None;
        self.hover_tab = None;
        if x < SIDEBAR_WIDTH && y >= TAB_TOP {
            let index = ((y - TAB_TOP) / TAB_HEIGHT) as usize;
            if index < self.tabs.len() {
                self.hover_tab = Some(index);
                if x >= SIDEBAR_WIDTH - 38 {
                    self.hover_close = Some(index);
                }
            }
        }
        if old_close != self.hover_close || old_tab != self.hover_tab {
            self.refresh();
        }
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

fn create_webview_controller(environment: &ICoreWebView2Environment, hwnd: HWND) -> AppResult<ICoreWebView2Controller> {
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
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            APP_NAME,
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            820,
            None,
            None,
            Some(hinstance),
            None,
        )?;
        Ok(hwnd)
    }
}

fn create_address_bar(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | WS_BORDER.0),
            SIDEBAR_WIDTH + 132,
            14,
            680,
            32,
            Some(parent),
            Some(HMENU(ADDRESS_ID as usize as *mut _)),
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        let _ = WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_SETMARGINS,
            Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
            Some(LPARAM((10 | (10 << 16)) as isize)),
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
                with_app(hwnd, |app| app.paint(hdc));
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
                        let _ = tab.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
                    }
                }
            });
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            unsafe {
                let hdc = HDC(w_param.0 as *mut _);
                let _ = SetTextColor(hdc, COLORREF(COLOR_TEXT));
                let _ = SetBkMode(hdc, TRANSPARENT);
                let brush = with_app_return(hwnd, |app| app.brushes.edit).unwrap_or_else(|| solid_brush(0x151515));
                LRESULT(brush.0 as isize)
            }
        }
        WM_SETCURSOR => unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) },
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
                let _ = app.create_tab("https://vercel.com");
            }
            0x57 if ctrl => app.close_tab(app.active),
            0x25 if alt => app.go_back(),
            0x27 if alt => app.go_forward(),
            code if code == VK_F5.0 as u32 => app.reload(),
            _ => {}
        });
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
            w!("Segoe UI"),
        ))
    }
}

fn draw_button(hdc: HDC, rect: RECT, label: &str, font: &HFONT) {
    unsafe {
        fill_round_rect(hdc, rect, COLOR_PANEL_2, 8);
        draw_outline(hdc, rect, COLOR_BORDER, 8);
        draw_text(hdc, font, label, rect, COLOR_TEXT);
    }
}

unsafe fn fill_round_rect(hdc: HDC, rect: RECT, color: u32, radius: i32) {
    let brush = solid_brush(color);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius, radius);
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn draw_outline(hdc: HDC, rect: RECT, color: u32, radius: i32) {
    let pen = CreatePen(Gdi::PS_SOLID, 1, COLORREF(color));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius, radius);
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
        return "https://vercel.com".to_string();
    }
    if value.contains("://") || value.starts_with("about:") {
        value.to_string()
    } else if value.contains('.') && !value.contains(' ') {
        format!("https://{value}")
    } else {
        format!("https://www.google.com/search?q={}", value.replace(' ', "+"))
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
