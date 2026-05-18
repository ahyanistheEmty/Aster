#![windows_subsystem = "windows"]

use std::{
    cell::{Cell, RefCell},
    fs, mem,
    path::PathBuf,
    ptr,
    sync::mpsc,
};

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
            self, AlphaBlend, BeginPaint, BitBlt, CreateBitmap, CreateCompatibleBitmap,
            CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateRectRgn,
            CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
            GetMonitorInfoW, GetStockObject, InvalidateRect, LineTo, MonitorFromWindow, MoveToEx,
            RoundRect, ScreenToClient, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
            AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
            DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
            HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            NULL_BRUSH, NULL_PEN, SRCCOPY, TRANSPARENT,
        },
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
        },
        System::{Com::*, LibraryLoader},
        UI::{
            Controls::{EM_SETMARGINS, EM_SETSEL},
            HiDpi,
            Input::KeyboardAndMouse::{
                GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_ESCAPE, VK_F11,
                VK_F5, VK_MENU, VK_RETURN,
            },
            WindowsAndMessaging::{
                self, CreateIconIndirect, GetCursorPos, GetTopWindow, GetWindow, CREATESTRUCTW,
                CW_USEDEFAULT, EC_LEFTMARGIN, EC_RIGHTMARGIN, GWLP_USERDATA, GWLP_WNDPROC,
                GWL_STYLE, GW_HWNDNEXT, HICON, HMENU, HWND_TOP, ICONINFO, ICON_BIG, ICON_SMALL,
                IDC_ARROW, MSG, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP,
                WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
                WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT, WM_RBUTTONDOWN,
                WM_SETCURSOR, WM_SETFOCUS, WM_SETFONT, WM_SETICON, WM_SIZE, WM_TIMER, WNDCLASSW,
                WNDPROC, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_DLGMODALFRAME, WS_OVERLAPPEDWINDOW,
                WS_POPUP, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
};

const APP_NAME: PCWSTR = w!("Aster");
const CLASS_NAME: PCWSTR = w!("AsterWindow");
const ADDRESS_ID: i32 = 1001;
const COMMAND_POPUP_ID: i32 = 1002;
const DEFAULT_URL: &str = "https://www.google.com";
const SIDEBAR_EXPANDED: f32 = 248.0;
const SIDEBAR_HIDDEN: f32 = 0.0;
const HOVER_ZONE: i32 = 8;
const TOPBAR_HEIGHT: i32 = 58;
const SIDEBAR_HEADER_TOP: i32 = TOPBAR_HEIGHT + 14;
const SIDEBAR_ROWS_TOP: i32 = TOPBAR_HEIGHT + 68;
const SIDEBAR_TIMER_ID: usize = 42;
const HOVER_LEAVE_TIMER_ID: usize = 43;
const HOVER_DETECT_TIMER_ID: usize = 44;
const STATE_FILE: &str = ".aster-state";
const MENU_TAB_PIN: usize = 3101;
const MENU_TAB_UNPIN: usize = 3102;
const MENU_TAB_REMOVE_FOLDER: usize = 3103;
const MENU_TAB_MOVE_FOLDER_BASE: usize = 3200;
const MENU_WORKSPACE_RENAME: usize = 3301;
const MENU_WORKSPACE_NEW_FOLDER: usize = 3302;
const MENU_WORKSPACE_NEW: usize = 3303;
const MENU_FOLDER_RENAME: usize = 3401;
const MENU_FOLDER_DELETE: usize = 3402;
const MENU_TAB_CLOSE: usize = 3501;
const MENU_TAB_NEW: usize = 3502;
const MENU_NEW_SPACE: usize = 3503;
const MENU_NEW_FOLDER: usize = 3504;
const MENU_FOLDER_PIN: usize = 3505;
const MENU_FOLDER_UNPIN: usize = 3506;
const MENU_HISTORY_BASE: usize = 3600;
const MENU_WIDTH: i32 = 270;
const MENU_ROW_HEIGHT: i32 = 34;

const COLOR_BLACK: u32 = 0x000000;
const COLOR_PANEL: u32 = 0x090909;
const COLOR_PANEL_2: u32 = 0x121212;
const COLOR_SURFACE_HOVER: u32 = 0x242424;
const COLOR_BORDER: u32 = 0x343434;
const COLOR_TEXT: u32 = 0xf5f5f5;
const COLOR_MUTED: u32 = 0xa1a1a1;
const COLOR_ACCENT: u32 = 0xf16f63;
const ASTER_BACKGROUND_SVG: &str = include_str!("../assets/aster-background.svg");

static mut OLD_ADDRESS_PROC: WNDPROC = None;
static mut OLD_COMMAND_POPUP_PROC: WNDPROC = None;

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

struct FaviconBitmap {
    handle: HBITMAP,
    width: i32,
    height: i32,
}

impl Drop for FaviconBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.handle.0));
        }
    }
}

struct BackgroundBitmap {
    handle: HBITMAP,
    width: i32,
    height: i32,
}

impl Drop for BackgroundBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.handle.0));
        }
    }
}

struct Workspace {
    id: usize,
    name: String,
}

struct Folder {
    id: usize,
    workspace_id: usize,
    name: String,
    collapsed: bool,
    pinned: bool,
}

struct Tab {
    id: usize,
    workspace_id: usize,
    folder_id: Option<usize>,
    pinned: bool,
    title: String,
    url: String,
    favicon_uri: String,
    favicon_bitmap: Option<FaviconBitmap>,
    audio_playing: bool,
    muted: bool,
    history: Vec<HistoryEntry>,
    history_cursor: usize,
    pending_history_jump: Option<usize>,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    child_hwnd: HWND,
}

#[derive(Clone)]
struct HistoryEntry {
    title: String,
    url: String,
}

#[derive(Clone)]
struct OverlayMenu {
    rect: RECT,
    target: MenuTarget,
    items: Vec<OverlayMenuItem>,
}

#[derive(Clone)]
struct OverlayMenuItem {
    id: usize,
    label: String,
    sublabel: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuTarget {
    Sidebar(SidebarHit),
    BackHistory(usize),
    ForwardHistory(usize),
    SidebarBlank,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragSource {
    Tab(usize),
    Folder(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DragState {
    source: DragSource,
    start_x: i32,
    start_y: i32,
    active: bool,
    current_x: i32,
    current_y: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarRow {
    Label(SidebarLabel),
    Folder(usize),
    Tab(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarLabel {
    Pinned,
    Tabs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarHit {
    WorkspaceHeader,
    WorkspaceButton(usize),
    AddButton,
    PinnedSection,
    Folder(usize),
    Tab(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoverTarget {
    Logo,
    NewTab,
    Address,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandMode {
    Navigate,
    NewTab,
    NewWorkspace,
    RenameWorkspace(usize),
    NewFolder,
    RenameFolder(usize),
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
    toolbar_icon: HFONT,
    url: HFONT,
}

impl Drop for UiFonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.body.0));
            let _ = DeleteObject(HGDIOBJ(self.small.0));
            let _ = DeleteObject(HGDIOBJ(self.icon.0));
            let _ = DeleteObject(HGDIOBJ(self.toolbar_icon.0));
            let _ = DeleteObject(HGDIOBJ(self.url.0));
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
    command_hwnd: HWND,
    environment: ICoreWebView2Environment,
    workspaces: Vec<Workspace>,
    folders: Vec<Folder>,
    tabs: Vec<Tab>,
    active_workspace: usize,
    active: usize,
    next_id: usize,
    next_workspace_id: usize,
    next_folder_id: usize,
    workspace_active_tabs: Vec<(usize, usize)>,
    loading_state: bool,
    fonts: UiFonts,
    brushes: UiBrushes,
    hover_close: Option<usize>,
    hover_tab: Option<usize>,
    hover_folder: Option<usize>,
    hover_target: Option<HoverTarget>,
    sidebar_width: f32,
    sidebar_target: f32,
    sidebar_mode: SidebarMode,
    sidebar_expand_mode: SidebarMode,
    animating_sidebar: bool,
    hovering_sidebar: bool,
    last_clip_width: Cell<f32>,
    last_bounds_rect: Cell<RECT>,
    site_mode: SiteMode,
    settings_open: bool,
    mode_menu_open: bool,
    overlay_menu: Option<OverlayMenu>,
    drag_state: Option<DragState>,
    drag_ghost: RefCell<Option<DragGhost>>,
    drop_target: Option<DropTarget>,
    background_cache: RefCell<Option<BackgroundBitmap>>,
    suggestions: Vec<String>,
    command_open: bool,
    command_mode: CommandMode,
    renaming_folder_id: Option<usize>,
    rename_buffer: String,
    fullscreen: bool,
    saved_style: isize,
    saved_rect: RECT,
}

struct DragGhost {
    handle: HBITMAP,
    width: i32,
    height: i32,
}

impl Drop for DragGhost {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.handle.0));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DropTarget {
    PinnedSection,
    Folder(usize),
    Tab(usize),
    None,
}

impl App {
    fn new(hwnd: HWND, environment: ICoreWebView2Environment) -> AppResult<Self> {
        let fonts = UiFonts {
            body: create_font(14, 400)?,
            small: create_font(12, 400)?,
            icon: create_font_with_face(18, 400, w!("Segoe Fluent Icons"))?,
            toolbar_icon: create_font_with_face(15, 400, w!("Segoe Fluent Icons"))?,
            url: create_font_with_face(13, 400, w!("Segoe UI Variable Text"))?,
        };
        let brushes = UiBrushes {
            black: solid_brush(COLOR_BLACK),
            panel: solid_brush(COLOR_PANEL),
            panel_2: solid_brush(COLOR_PANEL_2),
            edit: solid_brush(0x080808),
        };

        let address_hwnd = create_address_bar(hwnd)?;
        let command_hwnd = create_command_popup(hwnd)?;
        unsafe {
            let _ = WindowsAndMessaging::SendMessageW(
                address_hwnd,
                WM_SETFONT,
                Some(WPARAM(fonts.url.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        let mut app = Self {
            hwnd,
            address_hwnd,
            command_hwnd,
            environment,
            workspaces: vec![Workspace {
                id: 1,
                name: "Space 1".to_string(),
            }],
            folders: Vec::new(),
            tabs: Vec::new(),
            active_workspace: 1,
            active: 0,
            next_id: 1,
            next_workspace_id: 2,
            next_folder_id: 1,
            workspace_active_tabs: Vec::new(),
            loading_state: false,
            fonts,
            brushes,
            hover_close: None,
            hover_tab: None,
            hover_folder: None,
            hover_target: None,
            sidebar_width: SIDEBAR_HIDDEN,
            sidebar_target: SIDEBAR_HIDDEN,
            sidebar_mode: SidebarMode::Hidden,
            sidebar_expand_mode: SidebarMode::Hidden,
            animating_sidebar: false,
            hovering_sidebar: false,
            last_clip_width: Cell::new(0.0),
            last_bounds_rect: Cell::new(RECT {
                left: -1,
                top: -1,
                right: -1,
                bottom: -1,
            }),
            site_mode: SiteMode::Auto,
            settings_open: false,
            mode_menu_open: false,
            overlay_menu: None,
            drag_state: None,
            drag_ghost: RefCell::new(None),
            drop_target: Some(DropTarget::None),
            background_cache: RefCell::new(None),
            suggestions: Vec::new(),
            command_open: false,
            command_mode: CommandMode::Navigate,
            renaming_folder_id: None,
            rename_buffer: String::new(),
            fullscreen: false,
            saved_style: 0,
            saved_rect: RECT::default(),
        };
        app.load_state()?;
        unsafe {
            let _ = WindowsAndMessaging::SetTimer(Some(app.hwnd), HOVER_DETECT_TIMER_ID, 100, None);
        }
        Ok(app)
    }

    fn create_tab(&mut self, url: &str) -> AppResult<()> {
        self.create_tab_in_workspace(url, self.active_workspace, None, false, true, None)
    }

    fn create_tab_in_workspace(
        &mut self,
        url: &str,
        workspace_id: usize,
        folder_id: Option<usize>,
        pinned: bool,
        activate: bool,
        title: Option<String>,
    ) -> AppResult<()> {
        // Snapshot all direct children BEFORE creating the WebView2 controller
        // so we can reliably identify the new child window it creates.
        let children_before = collect_direct_children(self.hwnd);

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

        // Find the new child window that was created by this controller
        let child_hwnd = collect_direct_children(self.hwnd)
            .into_iter()
            .find(|h| !children_before.contains(h))
            .unwrap_or_default();

        self.tabs.push(Tab {
            id,
            workspace_id,
            folder_id,
            pinned,
            title: "New Tab".to_string(),
            url: url.to_string(),
            favicon_uri: String::new(),
            favicon_bitmap: None,
            audio_playing: false,
            muted: false,
            history: Vec::new(),
            history_cursor: 0,
            pending_history_jump: None,
            controller,
            webview,
            child_hwnd,
        });
        if let Some(title) = title {
            if let Some(tab) = self.tabs.get_mut(index) {
                if !title.trim().is_empty() {
                    tab.title = title;
                }
            }
        }
        if activate {
            self.active_workspace = workspace_id;
            self.switch_to(index);
        }
        let wide = CoTaskMemPWSTR::from(url);
        unsafe {
            let _ = self.tabs[index]
                .webview
                .Navigate(*wide.as_ref().as_pcwstr());
        }
        self.save_state();
        Ok(())
    }

    fn active_tab_index(&self) -> Option<usize> {
        self.tabs.get(self.active).and_then(|tab| {
            if tab.workspace_id == self.active_workspace {
                Some(self.active)
            } else {
                None
            }
        })
    }

    fn set_workspace_active_tab(&mut self, workspace_id: usize, tab_id: usize) {
        if let Some((_, active_tab)) = self
            .workspace_active_tabs
            .iter_mut()
            .find(|(id, _)| *id == workspace_id)
        {
            *active_tab = tab_id;
        } else {
            self.workspace_active_tabs.push((workspace_id, tab_id));
        }
    }

    fn switch_workspace(&mut self, workspace_id: usize) {
        if !self
            .workspaces
            .iter()
            .any(|workspace| workspace.id == workspace_id)
        {
            return;
        }
        self.active_workspace = workspace_id;
        for tab in &self.tabs {
            unsafe {
                let _ = tab.controller.SetIsVisible(false);
            }
        }
        let preferred_tab = self
            .workspace_active_tabs
            .iter()
            .find(|(id, _)| *id == workspace_id)
            .map(|(_, tab_id)| *tab_id);
        let next_index = preferred_tab
            .and_then(|tab_id| {
                self.tabs
                    .iter()
                    .position(|tab| tab.workspace_id == workspace_id && tab.id == tab_id)
            })
            .or_else(|| {
                self.tabs
                    .iter()
                    .position(|tab| tab.workspace_id == workspace_id)
            });
        if let Some(index) = next_index {
            self.switch_to(index);
        } else {
            self.active = 0;
            set_window_text(self.address_hwnd, "");
            self.layout();
            self.refresh();
            self.save_state();
            self.ensure_hover_detect_timer();
        }
    }

    fn switch_workspace_by_delta(&mut self, delta: i32) {
        if self.workspaces.len() < 2 {
            return;
        }
        let current = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id == self.active_workspace)
            .unwrap_or(0) as i32;
        let len = self.workspaces.len() as i32;
        let next = (current + delta).rem_euclid(len) as usize;
        if let Some(workspace) = self.workspaces.get(next) {
            self.switch_workspace(workspace.id);
        }
    }

    fn active_workspace_tabs(&self) -> Vec<usize> {
        self.sidebar_rows()
            .into_iter()
            .filter_map(|row| match row {
                SidebarRow::Tab(index) => Some(index),
                _ => None,
            })
            .collect()
    }

    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut rows = Vec::new();
        let pinned_folders: Vec<usize> = self
            .folders
            .iter()
            .filter(|folder| folder.workspace_id == self.active_workspace && folder.pinned)
            .map(|folder| folder.id)
            .collect();
        let pinned_tabs: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.workspace_id == self.active_workspace && tab.pinned)
            .map(|(index, _)| index)
            .collect();
        rows.push(SidebarRow::Label(SidebarLabel::Pinned));
        rows.extend(pinned_folders.into_iter().map(SidebarRow::Folder));
        rows.extend(pinned_tabs.into_iter().map(SidebarRow::Tab));

        let mut has_tabs_label = false;
        for folder in self
            .folders
            .iter()
            .filter(|folder| folder.workspace_id == self.active_workspace && !folder.pinned)
        {
            let folder_tabs: Vec<usize> = self
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, tab)| {
                    tab.workspace_id == self.active_workspace
                        && !tab.pinned
                        && tab.folder_id == Some(folder.id)
                })
                .map(|(index, _)| index)
                .collect();
            if !has_tabs_label {
                rows.push(SidebarRow::Label(SidebarLabel::Tabs));
                has_tabs_label = true;
            }
            rows.push(SidebarRow::Folder(folder.id));
            if !folder.collapsed {
                rows.extend(folder_tabs.into_iter().map(SidebarRow::Tab));
            }
        }

        let loose_tabs: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace && !tab.pinned && tab.folder_id.is_none()
            })
            .map(|(index, _)| index)
            .collect();
        if !loose_tabs.is_empty() {
            if !has_tabs_label {
                rows.push(SidebarRow::Label(SidebarLabel::Tabs));
            }
            rows.extend(loose_tabs.into_iter().map(SidebarRow::Tab));
        }
        rows
    }

    fn sidebar_row_rects(&self) -> Vec<(SidebarRow, RECT)> {
        let mut rects = Vec::new();
        let width = self.sidebar_width();
        if width <= 92 {
            return rects;
        }
        let bottom_limit = self.workspace_switcher_bounds().top - 10;
        let mut y = SIDEBAR_ROWS_TOP;
        for row in self.sidebar_rows() {
            let height = match row {
                SidebarRow::Label(_) => 24,
                SidebarRow::Folder(_) => 36,
                SidebarRow::Tab(_) => 44,
            };
            if y + height > bottom_limit {
                break;
            }
            rects.push((
                row,
                RECT {
                    left: 10,
                    top: y,
                    right: width - 10,
                    bottom: y + height,
                },
            ));
            y += height;
        }
        rects
    }

    fn workspace_header_rect(&self) -> RECT {
        RECT {
            left: 12,
            top: SIDEBAR_HEADER_TOP,
            right: self.sidebar_width() - 12,
            bottom: SIDEBAR_HEADER_TOP + 38,
        }
    }

    fn workspace_switcher_bounds(&self) -> RECT {
        let settings = self.settings_rect();
        RECT {
            left: 12,
            top: settings.top - 48,
            right: self.sidebar_width() - 12,
            bottom: settings.top - 14,
        }
    }

    fn workspace_switcher_items(&self) -> Vec<(SidebarHit, RECT)> {
        let bounds = self.workspace_switcher_bounds();
        let mut items = Vec::new();
        let mut x = bounds.left + 2;
        for workspace in &self.workspaces {
            let rect = RECT {
                left: x,
                top: bounds.top + 4,
                right: x + 28,
                bottom: bounds.top + 32,
            };
            if rect.right > bounds.right - 34 {
                break;
            }
            items.push((SidebarHit::WorkspaceButton(workspace.id), rect));
            x += 34;
        }
        items.push((
            SidebarHit::AddButton,
            RECT {
                left: (bounds.right - 30).max(bounds.left + 2),
                top: bounds.top + 4,
                right: bounds.right - 2,
                bottom: bounds.top + 32,
            },
        ));
        items
    }

    fn pinned_section_rect(&self) -> Option<RECT> {
        let width = self.sidebar_width();
        if width <= 92 {
            return None;
        }
        let rows = self.sidebar_rows();
        if rows.len() <= 1 {
            return None;
        }
        let pinned_count = rows.iter().take_while(|row| {
            matches!(
                row,
                SidebarRow::Label(SidebarLabel::Pinned)
                    | SidebarRow::Folder(_)
                    | SidebarRow::Tab(_)
            )
        }).count();
        if pinned_count <= 1 {
            let y = SIDEBAR_ROWS_TOP;
            let height = 24;
            Some(RECT {
                left: 10,
                top: y,
                right: width - 10,
                bottom: y + height,
            })
        } else {
            None
        }
    }

    fn hit_sidebar(&self, x: i32, y: i32) -> Option<SidebarHit> {
        if self.sidebar_width() <= 92 || (x as f32) >= self.sidebar_width {
            return None;
        }
        if point_in_rect(x, y, self.workspace_header_rect()) {
            return Some(SidebarHit::WorkspaceHeader);
        }
        for (hit, rect) in self.workspace_switcher_items() {
            if point_in_rect(x, y, rect) {
                return Some(hit);
            }
        }
        if self.pinned_section_rect().is_some() {
            if let Some(rect) = self.pinned_section_rect() {
                if point_in_rect(x, y, rect) {
                    return Some(SidebarHit::PinnedSection);
                }
            }
        }
        for (row, rect) in self.sidebar_row_rects() {
            if point_in_rect(x, y, rect) {
                return match row {
                    SidebarRow::Folder(id) => Some(SidebarHit::Folder(id)),
                    SidebarRow::Tab(index) => Some(SidebarHit::Tab(index)),
                    SidebarRow::Label(_) => None,
                };
            }
        }
        None
    }

    fn load_state(&mut self) -> AppResult<()> {
        let path = state_path();
        let Ok(raw) = fs::read_to_string(path) else {
            return Ok(());
        };
        self.loading_state = true;
        self.workspaces.clear();
        self.folders.clear();
        self.tabs.clear();
        self.workspace_active_tabs.clear();

        let mut tab_records = Vec::new();
        let mut active_workspace = 1usize;
        for line in raw.lines() {
            let parts: Vec<String> = line.split('\t').map(unescape_state).collect();
            if parts.is_empty() {
                continue;
            }
            match parts[0].as_str() {
                "workspace" if parts.len() >= 3 => {
                    if let Ok(id) = parts[1].parse::<usize>() {
                        self.workspaces.push(Workspace {
                            id,
                            name: parts[2].clone(),
                        });
                    }
                }
                "folder" if parts.len() >= 4 => {
                    if let (Ok(id), Ok(workspace_id)) =
                        (parts[1].parse::<usize>(), parts[2].parse::<usize>())
                    {
                        self.folders.push(Folder {
                            id,
                            workspace_id,
                            name: parts[3].clone(),
                            collapsed: parts.get(4).map(|value| value == "1").unwrap_or(false),
                            pinned: parts.get(5).map(|value| value == "1").unwrap_or(false),
                        });
                    }
                }
                "tab" if parts.len() >= 6 => {
                    if let Ok(workspace_id) = parts[1].parse::<usize>() {
                        let folder_id = if parts[2].is_empty() {
                            None
                        } else {
                            parts[2].parse::<usize>().ok()
                        };
                        let pinned = parts[3] == "1";
                        tab_records.push((
                            workspace_id,
                            folder_id,
                            pinned,
                            parts[4].clone(),
                            parts[5].clone(),
                            parts
                                .get(6)
                                .map(|raw| parse_history(raw))
                                .unwrap_or_default(),
                        ));
                    }
                }
                "suggestion" if parts.len() >= 2 => {
                    self.remember_suggestion(parts[1].clone());
                }
                "active_workspace" if parts.len() >= 2 => {
                    if let Ok(id) = parts[1].parse::<usize>() {
                        active_workspace = id;
                    }
                }
                "active_tab" if parts.len() >= 3 => {
                    if let (Ok(workspace_id), Ok(tab_id)) =
                        (parts[1].parse::<usize>(), parts[2].parse::<usize>())
                    {
                        self.workspace_active_tabs.push((workspace_id, tab_id));
                    }
                }
                _ => {}
            }
        }

        if self.workspaces.is_empty() {
            self.workspaces.push(Workspace {
                id: 1,
                name: "Space 1".to_string(),
            });
        }
        self.next_workspace_id = self
            .workspaces
            .iter()
            .map(|workspace| workspace.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.next_folder_id = self
            .folders
            .iter()
            .map(|folder| folder.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.active_workspace = if self
            .workspaces
            .iter()
            .any(|workspace| workspace.id == active_workspace)
        {
            active_workspace
        } else {
            self.workspaces[0].id
        };

        for (workspace_id, folder_id, pinned, title, url, history) in tab_records {
            if !url.trim().is_empty()
                && self
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.id == workspace_id)
            {
                let safe_folder = folder_id.filter(|id| {
                    self.folders
                        .iter()
                        .any(|folder| folder.id == *id && folder.workspace_id == workspace_id)
                });
                self.create_tab_in_workspace(
                    &url,
                    workspace_id,
                    safe_folder,
                    pinned,
                    false,
                    Some(title),
                )?;
                if let Some(tab) = self.tabs.last_mut() {
                    if !history.is_empty() {
                        tab.history = history;
                        tab.history_cursor = tab.history.len().saturating_sub(1);
                    }
                }
            }
        }
        self.loading_state = false;
        self.switch_workspace(self.active_workspace);
        Ok(())
    }

    fn save_state(&self) {
        if self.loading_state {
            return;
        }
        let mut lines = Vec::new();
        lines.push(format!("active_workspace\t{}", self.active_workspace));
        for (workspace_id, active_tab_id) in &self.workspace_active_tabs {
            lines.push(format!("active_tab\t{}\t{}", workspace_id, active_tab_id));
        }
        for workspace in &self.workspaces {
            lines.push(format!(
                "workspace\t{}\t{}",
                workspace.id,
                escape_state(&workspace.name)
            ));
        }
        for folder in &self.folders {
            lines.push(format!(
                "folder\t{}\t{}\t{}\t{}\t{}",
                folder.id,
                folder.workspace_id,
                escape_state(&folder.name),
                if folder.collapsed { "1" } else { "0" },
                if folder.pinned { "1" } else { "0" }
            ));
        }
        for tab in &self.tabs {
            if tab.url.trim().is_empty() {
                continue;
            }
            lines.push(format!(
                "tab\t{}\t{}\t{}\t{}\t{}\t{}",
                tab.workspace_id,
                tab.folder_id.map(|id| id.to_string()).unwrap_or_default(),
                if tab.pinned { "1" } else { "0" },
                escape_state(&tab.title),
                escape_state(&tab.url),
                serialize_history(&tab.history)
            ));
        }
        for suggestion in self.suggestions.iter().take(120) {
            lines.push(format!("suggestion\t{}", escape_state(suggestion)));
        }
        let _ = fs::write(state_path(), lines.join("\n"));
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

            if let Ok(webview15) = webview.cast::<ICoreWebView2_15>() {
                let hwnd = self.hwnd;
                let mut token = 0;
                webview15.add_FaviconChanged(
                    &FaviconChangedEventHandler::create(Box::new(move |sender, _args| {
                        if let Some(sender) = sender {
                            if let Ok(sender15) = sender.cast::<ICoreWebView2_15>() {
                                let mut uri = PWSTR::null();
                                if sender15.FaviconUri(&mut uri).is_ok() {
                                    let favicon_uri = CoTaskMemPWSTR::from(uri).to_string();
                                    with_app(hwnd, |app| {
                                        app.update_tab_favicon_uri(tab_id, favicon_uri)
                                    });
                                }
                                let hwnd = hwnd;
                                let _ = sender15.GetFavicon(
                                    COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG,
                                    &GetFaviconCompletedHandler::create(Box::new(
                                        move |error_code, stream| {
                                            if error_code.is_ok() {
                                                if let Some(stream) = stream {
                                                    if let Some(favicon) =
                                                        decode_favicon_stream(&stream)
                                                    {
                                                        with_app(hwnd, |app| {
                                                            app.update_tab_favicon_bitmap(
                                                                tab_id, favicon,
                                                            )
                                                        });
                                                    }
                                                }
                                            }
                                            Ok(())
                                        },
                                    )),
                                );
                            }
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
            }

            if let Ok(webview8) = webview.cast::<ICoreWebView2_8>() {
                let hwnd = self.hwnd;
                let mut token = 0;
                webview8.add_IsDocumentPlayingAudioChanged(
                    &IsDocumentPlayingAudioChangedEventHandler::create(Box::new(
                        move |sender, _args| {
                            if let Some(sender) = sender {
                                if let Ok(sender8) = sender.cast::<ICoreWebView2_8>() {
                                    let mut playing = BOOL::from(false);
                                    if sender8.IsDocumentPlayingAudio(&mut playing).is_ok() {
                                        with_app(hwnd, |app| {
                                            app.update_tab_audio(
                                                tab_id,
                                                Some(playing.as_bool()),
                                                None,
                                            )
                                        });
                                    }
                                }
                            }
                            Ok(())
                        },
                    )),
                    &mut token,
                )?;

                let hwnd = self.hwnd;
                let mut token = 0;
                webview8.add_IsMutedChanged(
                    &IsMutedChangedEventHandler::create(Box::new(move |sender, _args| {
                        if let Some(sender) = sender {
                            if let Ok(sender8) = sender.cast::<ICoreWebView2_8>() {
                                let mut muted = BOOL::from(false);
                                if sender8.IsMuted(&mut muted).is_ok() {
                                    with_app(hwnd, |app| {
                                        app.update_tab_audio(tab_id, None, Some(muted.as_bool()))
                                    });
                                }
                            }
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
            }

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
                if let Some(entry) = tab.history.get_mut(tab.history_cursor) {
                    entry.title = tab.title.clone();
                }
            }
        }
        self.save_state();
        self.refresh();
    }

    fn update_tab_url(&mut self, tab_id: usize, url: String) {
        let active_index = self.active_tab_index();
        let mut suggestion = None;
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
            if !tab.url.trim().is_empty() {
                let mut handled_history_jump = false;
                if let Some(target) = tab.pending_history_jump.take() {
                    if tab
                        .history
                        .get(target)
                        .map(|entry| entry.url == tab.url)
                        .unwrap_or(false)
                    {
                        tab.history_cursor = target;
                        handled_history_jump = true;
                    }
                }
                let should_push = !handled_history_jump
                    && tab
                        .history
                        .last()
                        .map(|entry| entry.url != tab.url)
                        .unwrap_or(true);
                if should_push {
                    tab.history.push(HistoryEntry {
                        title: tab.title.clone(),
                        url: tab.url.clone(),
                    });
                    tab.history_cursor = tab.history.len().saturating_sub(1);
                    if tab.history.len() > 80 {
                        let drain = tab.history.len() - 80;
                        tab.history.drain(0..drain);
                        tab.history_cursor = tab.history_cursor.saturating_sub(drain);
                    }
                }
                suggestion = Some(tab.url.clone());
            }
            if Some(index) == active_index {
                set_window_text(self.address_hwnd, &tab.url);
            }
        }
        if let Some(url) = suggestion {
            self.remember_suggestion(url);
        }
        self.save_state();
        self.refresh();
    }

    fn update_tab_favicon_uri(&mut self, tab_id: usize, favicon_uri: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.favicon_uri = favicon_uri;
        }
        self.refresh();
    }

    fn update_tab_favicon_bitmap(&mut self, tab_id: usize, favicon: FaviconBitmap) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.favicon_bitmap = Some(favicon);
        }
        self.refresh();
    }

    fn update_tab_audio(&mut self, tab_id: usize, playing: Option<bool>, muted: Option<bool>) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if let Some(playing) = playing {
                tab.audio_playing = playing;
            }
            if let Some(muted) = muted {
                tab.muted = muted;
            }
        }
        self.refresh();
    }

    fn remember_suggestion(&mut self, url: String) {
        let value = url.trim();
        if value.is_empty() || value == "about:blank" {
            return;
        }
        self.suggestions.retain(|item| item != value);
        self.suggestions.insert(0, value.to_string());
        if self.suggestions.len() > 120 {
            self.suggestions.truncate(120);
        }
    }

    fn switch_to(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let workspace_id = self.tabs[index].workspace_id;
        self.active_workspace = workspace_id;
        for (i, tab) in self.tabs.iter().enumerate() {
            unsafe {
                let _ = tab
                    .controller
                    .SetIsVisible(i == index && tab.workspace_id == workspace_id);
            }
        }
        self.active = index;
        self.set_workspace_active_tab(workspace_id, self.tabs[index].id);
        self.layout();
        if let Some(tab) = self.tabs.get(index) {
            set_window_text(self.address_hwnd, &tab.url);
            unsafe {
                let _ = tab
                    .controller
                    .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
        self.save_state();
        self.refresh();
        self.ensure_hover_detect_timer();
    }

    fn ensure_hover_detect_timer(&mut self) {
        if self.sidebar_mode == SidebarMode::Hidden && !self.animating_sidebar {
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
                let _ = WindowsAndMessaging::SetTimer(
                    Some(self.hwnd),
                    HOVER_DETECT_TIMER_ID,
                    100,
                    None,
                );
            }
        }
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.is_empty() || index >= self.tabs.len() {
            return;
        }
        let workspace_id = self.tabs[index].workspace_id;
        self.tabs.remove(index);
        if self.tabs.iter().all(|tab| tab.workspace_id != workspace_id) {
            self.active = 0;
            if self.active_workspace == workspace_id {
                set_window_text(self.address_hwnd, "");
            }
            self.layout();
            self.save_state();
            self.refresh();
            return;
        }
        if self.active_workspace == workspace_id {
            let ordered = self.active_workspace_tabs();
            let next = ordered
                .into_iter()
                .find(|candidate| *candidate >= index)
                .or_else(|| {
                    self.tabs
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, tab)| tab.workspace_id == workspace_id)
                        .map(|(index, _)| index)
                });
            if let Some(next) = next {
                self.switch_to(next);
            }
        } else {
            self.save_state();
            self.refresh();
            self.ensure_hover_detect_timer();
        }
    }

    fn navigate_active_from_address(&mut self) {
        if self.command_open {
            self.submit_command();
            return;
        }
        let raw = get_window_text(self.address_hwnd);
        let url = normalize_address(&raw);
        self.navigate_active(&url);
    }

    fn navigate_active(&mut self, url: &str) {
        let Some(index) = self.active_tab_index() else {
            let _ = self.create_tab(url);
            return;
        };
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.url = url.to_string();
            tab.title = label_for_url(url);
            set_window_text(self.address_hwnd, url);
            let wide = CoTaskMemPWSTR::from(url);
            unsafe {
                let _ = tab.webview.Navigate(*wide.as_ref().as_pcwstr());
            }
        }
        self.save_state();
        self.refresh();
    }

    fn open_command(&mut self, mode: CommandMode) {
        self.command_mode = mode;
        self.command_open = true;
        let initial_text = match mode {
            CommandMode::Navigate => self
                .active_tab_index()
                .and_then(|index| self.tabs.get(index))
                .map(|tab| tab.url.as_str())
                .unwrap_or(""),
            CommandMode::NewTab => "",
            CommandMode::NewWorkspace => "New Space",
            CommandMode::RenameWorkspace(id) => self
                .workspaces
                .iter()
                .find(|workspace| workspace.id == id)
                .map(|workspace| workspace.name.as_str())
                .unwrap_or("Space"),
            CommandMode::NewFolder => "New Folder",
            CommandMode::RenameFolder(id) => self
                .folders
                .iter()
                .find(|folder| folder.id == id)
                .map(|folder| folder.name.as_str())
                .unwrap_or("Folder"),
        };
        set_window_text(self.address_hwnd, initial_text);
        let cue = match mode {
            CommandMode::Navigate | CommandMode::NewTab => "Search or Enter URL...",
            CommandMode::NewWorkspace | CommandMode::RenameWorkspace(_) => "Workspace name...",
            CommandMode::NewFolder | CommandMode::RenameFolder(_) => "Folder name...",
        };
        set_edit_cue_banner(self.address_hwnd, cue);
        self.layout();
        unsafe {
            let _ =
                WindowsAndMessaging::ShowWindow(self.command_hwnd, WindowsAndMessaging::SW_SHOW);
            let _ =
                WindowsAndMessaging::ShowWindow(self.address_hwnd, WindowsAndMessaging::SW_SHOW);
            let _ = WindowsAndMessaging::SetWindowPos(
                self.command_hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                WindowsAndMessaging::SWP_NOMOVE
                    | WindowsAndMessaging::SWP_NOSIZE
                    | WindowsAndMessaging::SWP_NOACTIVATE,
            );
            let _ = WindowsAndMessaging::SetWindowPos(
                self.address_hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                WindowsAndMessaging::SWP_NOMOVE | WindowsAndMessaging::SWP_NOSIZE,
            );
            let _ = SetFocus(Some(self.address_hwnd));
            let _ = WindowsAndMessaging::SendMessageW(
                self.address_hwnd,
                EM_SETSEL,
                Some(WPARAM(0)),
                Some(LPARAM(-1)),
            );
            let _ = InvalidateRect(Some(self.command_hwnd), None, false);
        }
        self.refresh();
    }

    fn close_command(&mut self) {
        if !self.command_open {
            return;
        }
        self.command_open = false;
        self.layout();
        if let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        {
            unsafe {
                let _ = tab
                    .controller
                    .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
        self.refresh();
    }

    fn submit_command(&mut self) {
        let raw = get_window_text(self.address_hwnd);
        let url = normalize_address(&raw);
        let mode = self.command_mode;
        self.command_open = false;
        self.layout();
        match mode {
            CommandMode::Navigate => {
                if self.active_tab_index().is_none() {
                    let _ = self.create_tab(&url);
                } else {
                    self.navigate_active(&url);
                    if let Some(tab) = self
                        .active_tab_index()
                        .and_then(|index| self.tabs.get(index))
                    {
                        unsafe {
                            let _ = tab
                                .controller
                                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
                        }
                    }
                }
            }
            CommandMode::NewTab => {
                let _ = self.create_tab(&url);
            }
            CommandMode::NewWorkspace => {
                let name = raw.trim();
                let id = self.next_workspace_id;
                self.next_workspace_id += 1;
                self.workspaces.push(Workspace {
                    id,
                    name: if name.is_empty() {
                        format!("Space {}", self.workspaces.len() + 1)
                    } else {
                        name.to_string()
                    },
                });
                self.switch_workspace(id);
            }
            CommandMode::RenameWorkspace(id) => {
                let name = raw.trim();
                if !name.is_empty() {
                    if let Some(workspace) = self
                        .workspaces
                        .iter_mut()
                        .find(|workspace| workspace.id == id)
                    {
                        workspace.name = name.to_string();
                    }
                    self.save_state();
                    self.refresh();
                }
            }
            CommandMode::NewFolder => {
                let name = raw.trim();
                let id = self.next_folder_id;
                self.next_folder_id += 1;
                self.folders.push(Folder {
                    id,
                    workspace_id: self.active_workspace,
                    name: if name.is_empty() {
                        "New Folder".to_string()
                    } else {
                        name.to_string()
                    },
                    collapsed: false,
                    pinned: false,
                });
                self.save_state();
                self.refresh();
            }
            CommandMode::RenameFolder(id) => {
                let name = raw.trim();
                if !name.is_empty() {
                    if let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == id) {
                        folder.name = name.to_string();
                    }
                    self.save_state();
                    self.refresh();
                }
            }
        }
    }

    fn create_folder_inline(&mut self) {
        let id = self.next_folder_id;
        self.next_folder_id += 1;
        self.folders.push(Folder {
            id,
            workspace_id: self.active_workspace,
            name: "New Folder".to_string(),
            collapsed: false,
            pinned: false,
        });
        self.renaming_folder_id = Some(id);
        self.rename_buffer = "New Folder".to_string();
        self.save_state();
        self.refresh();
    }

    fn confirm_rename(&mut self) {
        if let Some(id) = self.renaming_folder_id.take() {
            let name = self.rename_buffer.clone();
            if !name.trim().is_empty() {
                if let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) {
                    folder.name = name.trim().to_string();
                }
            }
            self.rename_buffer.clear();
            self.save_state();
            self.refresh();
        }
    }

    fn cancel_rename(&mut self) {
        if let Some(id) = self.renaming_folder_id.take() {
            if let Some(folder) = self.folders.iter().find(|f| f.id == id) {
                if folder.name == "New Folder" {
                    self.folders.retain(|f| f.id != id);
                }
            }
            self.rename_buffer.clear();
            self.save_state();
            self.refresh();
        }
    }

    fn go_back(&self) {
        if let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        {
            unsafe {
                let mut can = BOOL::from(false);
                if tab.webview.CanGoBack(&mut can).is_ok() && can.as_bool() {
                    let _ = tab.webview.GoBack();
                }
            }
        }
    }

    fn go_forward(&self) {
        if let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        {
            unsafe {
                let mut can = BOOL::from(false);
                if tab.webview.CanGoForward(&mut can).is_ok() && can.as_bool() {
                    let _ = tab.webview.GoForward();
                }
            }
        }
    }

    fn reload(&self) {
        if let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        {
            unsafe {
                let _ = tab.webview.Reload();
            }
        }
    }

    fn toggle_tab_mute(&mut self, index: usize) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let next = !tab.muted;
        if let Ok(webview8) = tab.webview.cast::<ICoreWebView2_8>() {
            unsafe {
                let _ = webview8.SetIsMuted(next);
            }
        }
        tab.muted = next;
        self.refresh();
    }

    fn sidebar_width(&self) -> i32 {
        self.sidebar_width.round() as i32
    }

    fn top_button_x(&self) -> i32 {
        54
    }

    fn top_button_rects(&self) -> (RECT, RECT, RECT) {
        let x = self.top_button_x();
        (
            RECT {
                left: x,
                top: 16,
                right: x + 28,
                bottom: 44,
            },
            RECT {
                left: x + 38,
                top: 16,
                right: x + 66,
                bottom: 44,
            },
            RECT {
                left: x + 76,
                top: 16,
                right: x + 104,
                bottom: 44,
            },
        )
    }

    fn logo_rect(&self) -> RECT {
        RECT {
            left: 12,
            top: 13,
            right: 42,
            bottom: 43,
        }
    }

    fn new_tab_rect(&self) -> RECT {
        let (_, _, reload) = self.top_button_rects();
        RECT {
            left: reload.right + 10,
            top: 16,
            right: reload.right + 38,
            bottom: 44,
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

    fn tab_audio_rect(&self, item: RECT) -> RECT {
        RECT {
            left: item.right - 56,
            top: item.top + 8,
            right: item.right - 34,
            bottom: item.bottom - 8,
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

    fn address_pill_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let width = (rect.right - rect.left - 560).clamp(176, 258);
        let center = (rect.right + rect.left) / 2;
        RECT {
            left: center - width / 2,
            top: 11,
            right: center + width / 2,
            bottom: 43,
        }
    }

    fn command_popup_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let width = (rect.right - rect.left - 420).clamp(520, 800);
        let height = 228;
        let center = (rect.right + rect.left) / 2;
        let top = ((rect.bottom - rect.top) / 2 - height / 2).max(TOPBAR_HEIGHT + 42);
        RECT {
            left: center - width / 2,
            top,
            right: center + width / 2,
            bottom: top + height,
        }
    }

    fn command_input_rect(&self) -> RECT {
        let popup = self.command_popup_rect();
        RECT {
            left: popup.left + 54,
            top: popup.top + 22,
            right: popup.right - 124,
            bottom: popup.top + 48,
        }
    }

    fn command_tab_row_rect(&self, index: usize) -> RECT {
        let popup = self.command_popup_rect();
        let top = popup.top + 70 + index as i32 * 38;
        RECT {
            left: popup.left + 8,
            top,
            right: popup.right - 8,
            bottom: top + 34,
        }
    }

    fn command_suggestions(&self) -> Vec<(Option<usize>, String, String)> {
        let query = get_window_text(self.address_hwnd)
            .trim()
            .to_ascii_lowercase();
        let mut rows = Vec::new();
        for tab_index in self.active_workspace_tabs() {
            if let Some(tab) = self.tabs.get(tab_index) {
                if query.is_empty()
                    || tab.url.to_ascii_lowercase().starts_with(&query)
                    || tab.title.to_ascii_lowercase().contains(&query)
                {
                    rows.push((Some(tab_index), tab.title.clone(), tab.url.clone()));
                }
            }
            if rows.len() >= 5 {
                return rows;
            }
        }
        for url in &self.suggestions {
            if query.is_empty() || url.to_ascii_lowercase().starts_with(&query) {
                if rows.iter().any(|(_, _, row_url)| row_url == url) {
                    continue;
                }
                rows.push((None, label_for_url(url), url.clone()));
            }
            if rows.len() >= 5 {
                break;
            }
        }
        rows
    }

    fn new_tab_opacity(&self) -> f32 {
        1.0
    }

    fn layout(&self) {
        let rect = client_rect(self.hwnd);
        unsafe {
            let flags = WindowsAndMessaging::SWP_NOZORDER;
            if self.command_open {
                let popup = self.command_popup_rect();
                let input = self.command_input_rect();
                let _ = WindowsAndMessaging::SetWindowPos(
                    self.command_hwnd,
                    Some(HWND_TOP),
                    popup.left,
                    popup.top,
                    popup.right - popup.left,
                    popup.bottom - popup.top,
                    flags,
                );
                let _ = WindowsAndMessaging::SetWindowPos(
                    self.address_hwnd,
                    Some(HWND_TOP),
                    input.left,
                    input.top,
                    (input.right - input.left).max(120),
                    input.bottom - input.top,
                    flags,
                );
                let _ = WindowsAndMessaging::ShowWindow(
                    self.command_hwnd,
                    WindowsAndMessaging::SW_SHOW,
                );
                let _ = WindowsAndMessaging::ShowWindow(
                    self.address_hwnd,
                    WindowsAndMessaging::SW_SHOW,
                );
            } else {
                let _ = WindowsAndMessaging::ShowWindow(
                    self.address_hwnd,
                    WindowsAndMessaging::SW_HIDE,
                );
                let _ = WindowsAndMessaging::ShowWindow(
                    self.command_hwnd,
                    WindowsAndMessaging::SW_HIDE,
                );
            }
        }

        let sidebar_width = self.sidebar_width();
        let pushed_left = sidebar_width;
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
                            left: pushed_left,
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
                        left: 0,
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
                left: pushed_left,
                top: TOPBAR_HEIGHT,
                right: rect.right,
                bottom: rect.bottom,
            },
        };
        let needs_clipping = self.sidebar_mode == SidebarMode::Overlay
            || (self.sidebar_mode == SidebarMode::Hidden
                && self.sidebar_expand_mode == SidebarMode::Overlay
                && self.sidebar_target >= SIDEBAR_EXPANDED);
        let clip_changed = needs_clipping
            && sidebar_width > 0
            && (sidebar_width as f32 - self.last_clip_width.get()).abs() > 1.0;
        let was_clipped = self.last_clip_width.get() != 0.0;
        let should_clear = (!needs_clipping || sidebar_width <= 0) && was_clipped;
        for (i, tab) in self.tabs.iter().enumerate() {
            unsafe {
                let _ = tab.controller.SetBounds(bounds);
                if clip_changed {
                    let clip_left = sidebar_width;
                    let clip_top = 0;
                    let clip_right = rect.right;
                    let clip_bottom = rect.bottom - TOPBAR_HEIGHT;
                    let region = CreateRectRgn(clip_left, clip_top, clip_right, clip_bottom);
                    let _ = SetWindowRgn(tab.child_hwnd, Some(region), false);
                } else if should_clear {
                    let _ = SetWindowRgn(tab.child_hwnd, None, false);
                }
                let _ = tab
                    .controller
                    .SetIsVisible(Some(i) == self.active_tab_index());
            }
        }
        if clip_changed {
            self.last_clip_width.set(sidebar_width as f32);
        } else if should_clear {
            self.last_clip_width.set(0.0);
        }
        let last = self.last_bounds_rect.get();
        if bounds.left != last.left
            || bounds.right != last.right
            || bounds.top != last.top
            || bounds.bottom != last.bottom
        {
            self.last_bounds_rect.set(bounds);
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

            if self.active_tab_index().is_none() {
                self.paint_cached_background(
                    hdc,
                    RECT {
                        left: 0,
                        top: TOPBAR_HEIGHT,
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                );
            }

            if sidebar_width >= 1 {
                let sidebar = RECT {
                    left: 0,
                    top: TOPBAR_HEIGHT,
                    right: sidebar_width,
                    bottom: rect.bottom,
                };
                let _ = FillRect(hdc, &sidebar, self.brushes.panel);
                if !is_overlay {
                    fill_rect(
                        hdc,
                        RECT {
                            left: sidebar.right - 1,
                            top: TOPBAR_HEIGHT,
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
                draw_toolbar_icon_button(
                    hdc,
                    self.new_tab_rect(),
                    IconKind::Plus,
                    self.hover_target == Some(HoverTarget::NewTab),
                    &self.fonts.toolbar_icon,
                );
            }

            let (back, forward, reload) = self.top_button_rects();
            draw_toolbar_icon_button(
                hdc,
                back,
                IconKind::Back,
                self.hover_target == Some(HoverTarget::Back),
                &self.fonts.toolbar_icon,
            );
            draw_toolbar_icon_button(
                hdc,
                forward,
                IconKind::Forward,
                self.hover_target == Some(HoverTarget::Forward),
                &self.fonts.toolbar_icon,
            );
            draw_toolbar_icon_button(
                hdc,
                reload,
                IconKind::Reload,
                self.hover_target == Some(HoverTarget::Reload),
                &self.fonts.toolbar_icon,
            );

            let edit_rect = self.address_pill_rect();
            if self.hover_target == Some(HoverTarget::Address) || self.command_open {
                fill_rect(
                    hdc,
                    RECT {
                        left: edit_rect.left + 22,
                        top: edit_rect.bottom - 2,
                        right: edit_rect.right - 22,
                        bottom: edit_rect.bottom - 1,
                    },
                    COLOR_ACCENT,
                );
            }
            let address_label = self
                .active_tab_index()
                .and_then(|index| self.tabs.get(index))
                .map(|tab| {
                    if tab.url.is_empty() {
                        "Search or Enter URL..."
                    } else {
                        tab.url.as_str()
                    }
                })
                .unwrap_or("Search or Enter URL...");
            draw_centered_text(
                hdc,
                &self.fonts.small,
                address_label,
                RECT {
                    left: edit_rect.left + 14,
                    top: edit_rect.top,
                    right: edit_rect.right - 14,
                    bottom: edit_rect.bottom,
                },
                COLOR_TEXT,
            );

            draw_settings_button(
                hdc,
                self.settings_rect(),
                self.hover_target == Some(HoverTarget::Settings),
                &self.fonts.icon,
            );

            if sidebar_width > 92 {
                self.paint_workspace_header(hdc);
                for (row, row_rect) in self.sidebar_row_rects() {
                    match row {
                        SidebarRow::Label(label) => self.paint_sidebar_label(hdc, label, row_rect),
                        SidebarRow::Folder(folder_id) => {
                            self.paint_folder_row(hdc, folder_id, row_rect)
                        }
                        SidebarRow::Tab(index) => {
                            if let Some(tab) = self.tabs.get(index) {
                                self.paint_tab(hdc, index, tab, row_rect);
                            }
                        }
                    }
                }
                self.paint_drop_target_highlight(hdc);
                self.paint_workspace_switcher(hdc);
                self.paint_drag_ghost(hdc);
            }

            if self.settings_open {
                self.paint_settings_menu(hdc);
            }
            if let Some(menu) = &self.overlay_menu {
                self.paint_overlay_menu(hdc, menu);
            }
        }
    }

    fn paint_cached_background(&self, hdc: HDC, rect: RECT) {
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let needs_render = self
            .background_cache
            .borrow()
            .as_ref()
            .map(|bitmap| bitmap.width != width || bitmap.height != height)
            .unwrap_or(true);
        if needs_render {
            *self.background_cache.borrow_mut() = render_aster_background_bitmap(width, height);
        }
        if let Some(bitmap) = self.background_cache.borrow().as_ref() {
            unsafe {
                let mem_dc = CreateCompatibleDC(Some(hdc));
                if !mem_dc.is_invalid() {
                    let old = SelectObject(mem_dc, HGDIOBJ(bitmap.handle.0));
                    let _ = BitBlt(
                        hdc,
                        rect.left,
                        rect.top,
                        width,
                        height,
                        Some(mem_dc),
                        0,
                        0,
                        SRCCOPY,
                    );
                    let _ = SelectObject(mem_dc, old);
                    let _ = DeleteDC(mem_dc);
                }
            }
        }
    }

    fn paint_overlay_menu(&self, hdc: HDC, menu: &OverlayMenu) {
        unsafe {
            fill_round_rect(hdc, menu.rect, 0x111111, 10);
            draw_outline(hdc, menu.rect, 0x343434, 10);
            for (index, item) in menu.items.iter().enumerate() {
                let row = RECT {
                    left: menu.rect.left + 6,
                    top: menu.rect.top + 6 + index as i32 * MENU_ROW_HEIGHT,
                    right: menu.rect.right - 6,
                    bottom: menu.rect.top + 6 + (index as i32 + 1) * MENU_ROW_HEIGHT,
                };
                if self.hover_target.map(|_| false).unwrap_or(false) {
                    let _ = row;
                }
                draw_text(
                    hdc,
                    &self.fonts.small,
                    &item.label,
                    RECT {
                        left: row.left + 10,
                        top: row.top,
                        right: row.right - 10,
                        bottom: if item.sublabel.is_empty() {
                            row.bottom
                        } else {
                            row.top + 20
                        },
                    },
                    COLOR_TEXT,
                );
                if !item.sublabel.is_empty() {
                    draw_text(
                        hdc,
                        &self.fonts.small,
                        &item.sublabel,
                        RECT {
                            left: row.left + 10,
                            top: row.top + 15,
                            right: row.right - 10,
                            bottom: row.bottom,
                        },
                        COLOR_MUTED,
                    );
                }
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

    fn paint_command_popup(&self, hdc: HDC) {
        unsafe {
            let rect = client_rect(self.command_hwnd);
            let panel = RECT {
                left: 0,
                top: 0,
                right: rect.right - rect.left,
                bottom: rect.bottom - rect.top,
            };
            fill_round_rect(hdc, panel, 0x080808, 14);
            draw_outline(hdc, panel, COLOR_ACCENT, 14);

            let input = RECT {
                left: 18,
                top: 18,
                right: panel.right - 18,
                bottom: 52,
            };
            fill_round_rect(hdc, input, 0x080808, 10);
            draw_icon_glyph(
                hdc,
                &self.fonts.toolbar_icon,
                glyph(0xE721).as_str(),
                RECT {
                    left: input.left + 12,
                    top: input.top,
                    right: input.left + 34,
                    bottom: input.bottom,
                },
                COLOR_TEXT,
            );

            fill_rect(
                hdc,
                RECT {
                    left: panel.left + 1,
                    top: 62,
                    right: panel.right - 1,
                    bottom: 63,
                },
                0x222222,
            );

            for (row_index, (tab_index, title, _url)) in
                self.command_suggestions().into_iter().take(5).enumerate()
            {
                let mut row = self.command_tab_row_rect(row_index);
                row.left -= self.command_popup_rect().left;
                row.right -= self.command_popup_rect().left;
                row.top -= self.command_popup_rect().top;
                row.bottom -= self.command_popup_rect().top;
                if tab_index == self.active_tab_index() {
                    fill_round_rect(hdc, row, COLOR_ACCENT, 8);
                }
                let favicon = RECT {
                    left: row.left + 14,
                    top: row.top + 8,
                    right: row.left + 30,
                    bottom: row.top + 24,
                };
                if let Some(index) = tab_index.and_then(|index| self.tabs.get(index)) {
                    draw_tab_favicon(hdc, &self.fonts.small, favicon, index);
                } else {
                    draw_icon_glyph(
                        hdc,
                        &self.fonts.toolbar_icon,
                        glyph(0xE774).as_str(),
                        favicon,
                        COLOR_ACCENT,
                    );
                }
                draw_text(
                    hdc,
                    &self.fonts.body,
                    &title,
                    RECT {
                        left: row.left + 42,
                        top: row.top,
                        right: row.right - 142,
                        bottom: row.bottom,
                    },
                    COLOR_TEXT,
                );
                draw_text(
                    hdc,
                    &self.fonts.small,
                    if tab_index == self.active_tab_index() {
                        "Active"
                    } else if tab_index.is_some() {
                        "Switch to Tab"
                    } else {
                        "Open"
                    },
                    RECT {
                        left: row.right - 118,
                        top: row.top,
                        right: row.right - 28,
                        bottom: row.bottom,
                    },
                    if tab_index == self.active_tab_index() {
                        COLOR_TEXT
                    } else {
                        COLOR_MUTED
                    },
                );
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    glyph(0xE72A).as_str(),
                    RECT {
                        left: row.right - 28,
                        top: row.top,
                        right: row.right - 8,
                        bottom: row.bottom,
                    },
                    if tab_index == self.active_tab_index() {
                        COLOR_TEXT
                    } else {
                        COLOR_MUTED
                    },
                );
            }
        }
    }

    fn paint_workspace_header(&self, hdc: HDC) {
        unsafe {
            let rect = self.workspace_header_rect();
            fill_round_rect(
                hdc,
                RECT {
                    left: rect.left + 12,
                    top: rect.top + 17,
                    right: rect.right - 12,
                    bottom: rect.top + 18,
                },
                0x242424,
                1,
            );
        }
    }

    fn paint_sidebar_label(&self, hdc: HDC, label: SidebarLabel, rect: RECT) {
        unsafe {
            let left = match label {
                SidebarLabel::Pinned => rect.left + 12,
                SidebarLabel::Tabs => rect.left + 22,
            };
            fill_rect(
                hdc,
                RECT {
                    left,
                    top: rect.top + 12,
                    right: rect.right - 8,
                    bottom: rect.top + 13,
                },
                0x2a2a2a,
            );
            if label == SidebarLabel::Pinned {
                let has_pinned_content = self
                    .sidebar_rows()
                    .iter()
                    .skip(1)
                    .take_while(|row| {
                        matches!(
                            row,
                            SidebarRow::Folder(_) | SidebarRow::Tab(_)
                        )
                    })
                    .count() > 0;
                if !has_pinned_content {
                    let workspace_name = self
                        .workspaces
                        .iter()
                        .find(|w| w.id == self.active_workspace)
                        .map(|w| w.name.as_str())
                        .unwrap_or("Space");
                    draw_text(
                        hdc,
                        &self.fonts.small,
                        workspace_name,
                        RECT {
                            left: rect.left + 12,
                            top: rect.top - 2,
                            right: rect.right - 8,
                            bottom: rect.top + 12,
                        },
                        COLOR_MUTED,
                    );
                }
            }
        }
    }

    fn paint_folder_row(&self, hdc: HDC, folder_id: usize, rect: RECT) {
        let Some(folder) = self.folders.iter().find(|folder| folder.id == folder_id) else {
            return;
        };
        let is_renaming = self.renaming_folder_id == Some(folder_id);
        unsafe {
            let item = RECT {
                left: rect.left + 2,
                top: rect.top + 2,
                right: rect.right - 2,
                bottom: rect.bottom - 2,
            };
            if is_renaming {
                fill_round_rect(hdc, item, 0x242424, 8);
            } else if self.hover_folder == Some(folder_id) {
                fill_round_rect(hdc, item, 0x151515, 8);
            }
            let folder_arrow = if folder.collapsed {
                glyph(0xE76C)
            } else {
                glyph(0xE70D)
            };
            let icon_left = if folder.pinned { 6 } else { 6 };
            draw_icon_glyph(
                hdc,
                &self.fonts.toolbar_icon,
                folder_arrow.as_str(),
                RECT {
                    left: item.left + icon_left,
                    top: item.top,
                    right: item.left + icon_left + 18,
                    bottom: item.bottom,
                },
                COLOR_MUTED,
            );
            if folder.pinned {
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    glyph(0xE718).as_str(),
                    RECT {
                        left: item.left + 28,
                        top: item.top,
                        right: item.left + 50,
                        bottom: item.bottom,
                    },
                    COLOR_ACCENT,
                );
                draw_text(
                    hdc,
                    &self.fonts.body,
                    if is_renaming { &self.rename_buffer } else { &folder.name },
                    RECT {
                        left: item.left + 56,
                        top: item.top,
                        right: item.right - 8,
                        bottom: item.bottom,
                    },
                    if is_renaming { COLOR_TEXT } else { COLOR_MUTED },
                );
            } else {
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    glyph(0xE8B7).as_str(),
                    RECT {
                        left: item.left + 28,
                        top: item.top,
                        right: item.left + 50,
                        bottom: item.bottom,
                    },
                    COLOR_MUTED,
                );
                draw_text(
                    hdc,
                    &self.fonts.body,
                    if is_renaming { &self.rename_buffer } else { &folder.name },
                    RECT {
                        left: item.left + 56,
                        top: item.top,
                        right: item.right - 8,
                        bottom: item.bottom,
                    },
                    if is_renaming { COLOR_TEXT } else { COLOR_MUTED },
                );
            }
        }
    }

    fn paint_workspace_switcher(&self, hdc: HDC) {
        unsafe {
            for (hit, rect) in self.workspace_switcher_items() {
                match hit {
                    SidebarHit::WorkspaceButton(id) => {
                        let active = id == self.active_workspace;
                        fill_round_rect(
                            hdc,
                            rect,
                            if active { COLOR_ACCENT } else { 0x151515 },
                            14,
                        );
                        draw_outline(hdc, rect, if active { COLOR_ACCENT } else { 0x2f2f2f }, 14);
                        let label = self
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.id == id)
                            .and_then(|workspace| workspace.name.chars().next())
                            .unwrap_or('S')
                            .to_ascii_uppercase()
                            .to_string();
                        draw_centered_text(
                            hdc,
                            &self.fonts.small,
                            &label,
                            rect,
                            if active { COLOR_TEXT } else { COLOR_MUTED },
                        );
                    }
                    SidebarHit::AddButton => {
                        draw_icon_glyph(
                            hdc,
                            &self.fonts.toolbar_icon,
                            glyph(0xE710).as_str(),
                            rect,
                            COLOR_TEXT,
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    fn paint_drop_target_highlight(&self, hdc: HDC) {
        if !self.drag_state.as_ref().map(|d| d.active).unwrap_or(false) {
            return;
        }
        unsafe {
            match self.drop_target {
                Some(DropTarget::PinnedSection) => {
                    if let Some(rect) = self.pinned_section_rect() {
                        let line_rect = RECT {
                            left: rect.left + 4,
                            top: rect.bottom - 2,
                            right: rect.right - 4,
                            bottom: rect.bottom,
                        };
                        fill_rect(hdc, line_rect, COLOR_ACCENT);
                    }
                }
                Some(DropTarget::Folder(folder_id)) => {
                    if let Some((_, rect)) = self
                        .sidebar_row_rects()
                        .into_iter()
                        .find(|(row, _)| matches!(row, SidebarRow::Folder(id) if *id == folder_id))
                    {
                        let line_rect = RECT {
                            left: rect.left + 4,
                            top: rect.bottom - 2,
                            right: rect.right - 4,
                            bottom: rect.bottom,
                        };
                        fill_rect(hdc, line_rect, COLOR_ACCENT);
                    }
                }
                Some(DropTarget::Tab(index)) => {
                    if let Some((_, rect)) = self
                        .sidebar_row_rects()
                        .into_iter()
                        .find(|(row, _)| matches!(row, SidebarRow::Tab(idx) if *idx == index))
                    {
                        let line_rect = RECT {
                            left: rect.left + 4,
                            top: rect.bottom - 2,
                            right: rect.right - 4,
                            bottom: rect.bottom,
                        };
                        fill_rect(hdc, line_rect, COLOR_ACCENT);
                    }
                }
                Some(DropTarget::None) | None => {}
            }
        }
    }

    fn paint_drag_ghost(&self, hdc: HDC) {
        if !self.drag_state.as_ref().map(|d| d.active).unwrap_or(false) {
            return;
        }
        let binding = self.drag_ghost.borrow();
        let Some(ghost) = binding.as_ref() else {
            return;
        };
        let drag = self.drag_state.unwrap();
        unsafe {
            let mem_dc = CreateCompatibleDC(Some(hdc));
            let old = SelectObject(mem_dc, HGDIOBJ(ghost.handle.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 180,
                AlphaFormat: 0,
            };
            let _ = AlphaBlend(
                hdc,
                drag.current_x + 10,
                drag.current_y + 10,
                ghost.width,
                ghost.height,
                mem_dc,
                0,
                0,
                ghost.width,
                ghost.height,
                blend,
            );
            SelectObject(mem_dc, old);
            let _ = DeleteDC(mem_dc);
        }
    }

    fn paint_tab(&self, hdc: HDC, index: usize, tab: &Tab, item: RECT) {
        unsafe {
            let mut item = item;
            if tab.folder_id.is_some() && !tab.pinned {
                item.left += 14;
            }
            if self.hover_tab == Some(index) || Some(index) == self.active_tab_index() {
                fill_round_rect(hdc, item, 0x151515, 10);
            }
            let mut text_left = item.left + 40;
            if tab.pinned {
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    glyph(0xE718).as_str(),
                    RECT {
                        left: item.left + 8,
                        top: item.top,
                        right: item.left + 28,
                        bottom: item.bottom,
                    },
                    COLOR_ACCENT,
                );
                text_left = item.left + 62;
            }
            let favicon_left = if tab.pinned {
                item.left + 34
            } else {
                item.left + 12
            };
            let favicon = RECT {
                left: favicon_left,
                top: item.top + 11,
                right: favicon_left + 18,
                bottom: item.top + 29,
            };
            draw_tab_favicon(hdc, &self.fonts.small, favicon, tab);
            draw_text(
                hdc,
                &self.fonts.body,
                &tab.title,
                RECT {
                    left: text_left,
                    top: item.top,
                    right: if tab.audio_playing || tab.muted {
                        item.right - 62
                    } else {
                        item.right - 36
                    },
                    bottom: item.bottom,
                },
                if Some(index) == self.active_tab_index() {
                    COLOR_TEXT
                } else {
                    COLOR_MUTED
                },
            );
            if tab.audio_playing || tab.muted {
                let audio_icon = if tab.muted {
                    glyph(0xE74F)
                } else {
                    glyph(0xE767)
                };
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    audio_icon.as_str(),
                    self.tab_audio_rect(item),
                    if tab.muted { COLOR_MUTED } else { COLOR_TEXT },
                );
            }
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
        if self.overlay_menu.is_some() && self.handle_overlay_click(x, y) {
            return;
        }

        if self.command_open
            && !point_in_rect(x, y, self.command_popup_rect())
            && !point_in_rect(x, y, self.address_pill_rect())
        {
            self.close_command();
            return;
        }

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
            self.open_command(CommandMode::NewTab);
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

        if point_in_rect(x, y, self.address_pill_rect()) {
            self.open_command(CommandMode::Navigate);
            return;
        }

        if point_in_rect(x, y, self.settings_rect()) {
            self.settings_open = !self.settings_open;
            self.mode_menu_open = false;
            self.refresh();
            return;
        }

        if let Some(hit) = self.hit_sidebar(x, y) {
            match hit {
                SidebarHit::WorkspaceHeader => {}
                SidebarHit::WorkspaceButton(id) => self.switch_workspace(id),
                SidebarHit::AddButton => self.open_new_item_menu(x, y),
                SidebarHit::PinnedSection => {}
                SidebarHit::Folder(folder_id) => {
                    if self.renaming_folder_id == Some(folder_id) {
                        return;
                    }
                    if let Some(folder) = self
                        .folders
                        .iter_mut()
                        .find(|folder| folder.id == folder_id)
                    {
                        folder.collapsed = !folder.collapsed;
                        self.save_state();
                        self.refresh();
                    }
                }
                SidebarHit::Tab(index) => {
                    let row =
                        self.sidebar_row_rects()
                            .into_iter()
                            .find_map(|(row, rect)| match row {
                                SidebarRow::Tab(row_index) if row_index == index => Some(rect),
                                _ => None,
                            });
                    if let Some(row) = row {
                        if self
                            .tabs
                            .get(index)
                            .map(|tab| tab.audio_playing || tab.muted)
                            .unwrap_or(false)
                            && point_in_rect(x, y, self.tab_audio_rect(row))
                        {
                            self.toggle_tab_mute(index);
                        } else if x >= row.right - 34 {
                            self.close_tab(index);
                        } else if index != self.active {
                            self.switch_to(index);
                        }
                    }
                }
            }
        }
    }

    fn handle_right_click(&mut self, x: i32, y: i32) {
        let (back, forward, _) = self.top_button_rects();
        if point_in_rect(x, y, back) {
            self.open_history_menu(x, y, true);
            return;
        }
        if point_in_rect(x, y, forward) {
            self.open_history_menu(x, y, false);
            return;
        }

        let Some(hit) = self.hit_sidebar(x, y) else {
            if self.sidebar_width() > 92 && (x as f32) < self.sidebar_width {
                self.open_sidebar_blank_menu(x, y);
            }
            return;
        };
        match hit {
            SidebarHit::WorkspaceHeader | SidebarHit::WorkspaceButton(_) | SidebarHit::PinnedSection => {
                let workspace_id = match hit {
                    SidebarHit::WorkspaceButton(id) => id,
                    _ => self.active_workspace,
                };
                self.open_overlay_menu(
                    x,
                    y,
                    MenuTarget::Sidebar(SidebarHit::WorkspaceButton(workspace_id)),
                    vec![
                        menu_item(MENU_WORKSPACE_RENAME, "Rename Workspace"),
                        menu_item(MENU_WORKSPACE_NEW_FOLDER, "New Folder"),
                        menu_item(MENU_WORKSPACE_NEW, "New Workspace"),
                        menu_item(MENU_TAB_NEW, "New Tab"),
                    ],
                );
            }
            SidebarHit::AddButton => self.open_new_item_menu(x, y),
            SidebarHit::Folder(folder_id) => {
                let folder = self.folders.iter().find(|f| f.id == folder_id);
                let is_pinned = folder.map(|f| f.pinned).unwrap_or(false);
                self.open_overlay_menu(
                    x,
                    y,
                    MenuTarget::Sidebar(SidebarHit::Folder(folder_id)),
                    vec![
                        menu_item(
                            if is_pinned { MENU_FOLDER_UNPIN } else { MENU_FOLDER_PIN },
                            if is_pinned { "Unpin Folder" } else { "Pin Folder" },
                        ),
                        menu_item(MENU_FOLDER_RENAME, "Rename Folder"),
                        menu_item(MENU_FOLDER_DELETE, "Remove Folder"),
                        menu_item(MENU_TAB_NEW, "New Tab"),
                    ],
                );
            }
            SidebarHit::Tab(index) => {
                let Some(tab) = self.tabs.get(index) else {
                    return;
                };
                let folders: Vec<(usize, String)> = self
                    .folders
                    .iter()
                    .filter(|folder| folder.workspace_id == tab.workspace_id)
                    .map(|folder| (folder.id, folder.name.clone()))
                    .collect();
                let mut labels: Vec<(usize, String)> = Vec::new();
                labels.push((
                    if tab.pinned {
                        MENU_TAB_UNPIN
                    } else {
                        MENU_TAB_PIN
                    },
                    if tab.pinned {
                        "Unpin Tab".to_string()
                    } else {
                        "Pin Tab".to_string()
                    },
                ));
                labels.push((MENU_WORKSPACE_NEW_FOLDER, "New Folder".to_string()));
                if tab.folder_id.is_some() {
                    labels.push((MENU_TAB_REMOVE_FOLDER, "Remove From Folder".to_string()));
                }
                for (offset, (_, name)) in folders.iter().enumerate() {
                    labels.push((
                        MENU_TAB_MOVE_FOLDER_BASE + offset,
                        format!("Move to {}", name),
                    ));
                }
                labels.push((MENU_TAB_CLOSE, "Close Tab".to_string()));
                let items: Vec<OverlayMenuItem> = labels
                    .iter()
                    .map(|(id, label)| menu_item(*id, label))
                    .collect();
                self.open_overlay_menu(x, y, MenuTarget::Sidebar(SidebarHit::Tab(index)), items);
            }
        }
    }

    fn open_sidebar_blank_menu(&mut self, x: i32, y: i32) {
        self.open_overlay_menu(
            x,
            y,
            MenuTarget::SidebarBlank,
            vec![
                menu_item(MENU_TAB_NEW, "New Tab"),
                menu_item(MENU_WORKSPACE_NEW_FOLDER, "New Folder"),
                menu_item(MENU_WORKSPACE_NEW, "New Workspace"),
                menu_item(MENU_WORKSPACE_RENAME, "Rename Workspace"),
            ],
        );
    }

    fn open_new_item_menu(&mut self, x: i32, y: i32) {
        self.open_overlay_menu(
            x,
            y,
            MenuTarget::Sidebar(SidebarHit::AddButton),
            vec![
                menu_item(MENU_NEW_SPACE, "New Space"),
                menu_item(MENU_NEW_FOLDER, "New Folder"),
            ],
        );
    }

    fn open_overlay_menu(
        &mut self,
        x: i32,
        y: i32,
        target: MenuTarget,
        items: Vec<OverlayMenuItem>,
    ) {
        if items.is_empty() {
            self.overlay_menu = None;
            return;
        }
        let rect = client_rect(self.hwnd);
        let height = 12 + items.len() as i32 * MENU_ROW_HEIGHT;
        let left = x.min(rect.right - MENU_WIDTH - 8).max(8);
        let top = y.min(rect.bottom - height - 8).max(TOPBAR_HEIGHT + 8);
        self.overlay_menu = Some(OverlayMenu {
            rect: RECT {
                left,
                top,
                right: left + MENU_WIDTH,
                bottom: top + height,
            },
            target,
            items,
        });
        self.refresh();
    }

    fn open_history_menu(&mut self, x: i32, y: i32, back: bool) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let mut items = Vec::new();
        if back {
            for (offset, entry) in tab.history.iter().rev().skip(1).take(8).enumerate() {
                let title = if entry.title.trim().is_empty() {
                    label_for_url(&entry.url)
                } else {
                    entry.title.clone()
                };
                items.push(menu_item_with_subtitle(
                    MENU_HISTORY_BASE + offset,
                    &title,
                    &entry.url,
                ));
            }
        } else if tab.history_cursor + 1 < tab.history.len() {
            for (offset, entry) in tab
                .history
                .iter()
                .skip(tab.history_cursor + 1)
                .take(8)
                .enumerate()
            {
                let title = if entry.title.trim().is_empty() {
                    label_for_url(&entry.url)
                } else {
                    entry.title.clone()
                };
                items.push(menu_item_with_subtitle(
                    MENU_HISTORY_BASE + offset,
                    &title,
                    &entry.url,
                ));
            }
        }
        self.open_overlay_menu(
            x,
            y,
            if back {
                MenuTarget::BackHistory(index)
            } else {
                MenuTarget::ForwardHistory(index)
            },
            items,
        );
    }

    fn handle_overlay_click(&mut self, x: i32, y: i32) -> bool {
        let Some(menu) = self.overlay_menu.clone() else {
            return false;
        };
        if !point_in_rect(x, y, menu.rect) {
            self.overlay_menu = None;
            self.refresh();
            return true;
        }
        let row_index = (y - menu.rect.top - 6) / MENU_ROW_HEIGHT;
        if row_index < 0 || row_index as usize >= menu.items.len() {
            return true;
        }
        let id = menu.items[row_index as usize].id;
        self.overlay_menu = None;
        self.run_menu_command(menu.target, id);
        true
    }

    fn run_menu_command(&mut self, target: MenuTarget, id: usize) {
        match target {
            MenuTarget::Sidebar(hit) => self.run_sidebar_menu_command(hit, id),
            MenuTarget::SidebarBlank => {
                self.run_sidebar_menu_command(SidebarHit::WorkspaceHeader, id)
            }
            MenuTarget::BackHistory(index) => self.navigate_to_history_item(index, id, true),
            MenuTarget::ForwardHistory(index) => self.navigate_to_history_item(index, id, false),
        }
        self.refresh();
    }

    fn run_sidebar_menu_command(&mut self, hit: SidebarHit, id: usize) {
        match id {
            MENU_TAB_NEW => self.open_command(CommandMode::NewTab),
            MENU_NEW_SPACE => self.open_command(CommandMode::NewWorkspace),
            MENU_NEW_FOLDER => self.create_folder_inline(),
            MENU_WORKSPACE_NEW => self.open_command(CommandMode::NewWorkspace),
            MENU_WORKSPACE_NEW_FOLDER => self.open_command(CommandMode::NewFolder),
            MENU_WORKSPACE_RENAME => {
                let workspace_id = match hit {
                    SidebarHit::WorkspaceButton(id) => id,
                    _ => self.active_workspace,
                };
                self.open_command(CommandMode::RenameWorkspace(workspace_id));
            }
            MENU_FOLDER_RENAME => {
                if let SidebarHit::Folder(folder_id) = hit {
                    if self.renaming_folder_id.is_some() {
                        return;
                    }
                    self.open_command(CommandMode::RenameFolder(folder_id));
                }
            }
            MENU_FOLDER_DELETE => {
                if let SidebarHit::Folder(folder_id) = hit {
                    self.folders.retain(|folder| folder.id != folder_id);
                    for tab in &mut self.tabs {
                        if tab.folder_id == Some(folder_id) {
                            tab.folder_id = None;
                        }
                    }
                    self.save_state();
                }
            }
            MENU_FOLDER_PIN => {
                if let SidebarHit::Folder(folder_id) = hit {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.id == folder_id) {
                        folder.pinned = true;
                    }
                    self.save_state();
                }
            }
            MENU_FOLDER_UNPIN => {
                if let SidebarHit::Folder(folder_id) = hit {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.id == folder_id) {
                        folder.pinned = false;
                    }
                    self.save_state();
                }
            }
            MENU_TAB_PIN | MENU_TAB_UNPIN | MENU_TAB_REMOVE_FOLDER | MENU_TAB_CLOSE
                if matches!(hit, SidebarHit::Tab(_)) =>
            {
                if let SidebarHit::Tab(index) = hit {
                    match id {
                        MENU_TAB_PIN => {
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.pinned = true;
                                tab.folder_id = None;
                            }
                        }
                        MENU_TAB_UNPIN => {
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.pinned = false;
                            }
                        }
                        MENU_TAB_REMOVE_FOLDER => {
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.folder_id = None;
                            }
                        }
                        MENU_TAB_CLOSE => self.close_tab(index),
                        _ => {}
                    }
                    self.save_state();
                }
            }
            id if id >= MENU_TAB_MOVE_FOLDER_BASE => {
                if let SidebarHit::Tab(index) = hit {
                    let tab_workspace = self.tabs.get(index).map(|tab| tab.workspace_id);
                    if let Some(workspace_id) = tab_workspace {
                        let folders: Vec<usize> = self
                            .folders
                            .iter()
                            .filter(|folder| folder.workspace_id == workspace_id)
                            .map(|folder| folder.id)
                            .collect();
                        let offset = id - MENU_TAB_MOVE_FOLDER_BASE;
                        if let Some(folder_id) = folders.get(offset).copied() {
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.pinned = false;
                                tab.folder_id = Some(folder_id);
                            }
                            self.save_state();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn navigate_to_history_item(&mut self, index: usize, id: usize, back: bool) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let offset = id.saturating_sub(MENU_HISTORY_BASE);
        let target = if back {
            tab.history.len().checked_sub(2 + offset)
        } else {
            Some(tab.history_cursor + 1 + offset).filter(|target| *target < tab.history.len())
        };
        if let Some(target) = target {
            if let Some(entry) = tab.history.get(target).cloned() {
                tab.history_cursor = target;
                tab.pending_history_jump = Some(target);
                tab.url = entry.url.clone();
                let wide = CoTaskMemPWSTR::from(entry.url.as_str());
                unsafe {
                    let _ = tab.webview.Navigate(*wide.as_ref().as_pcwstr());
                }
            }
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        if let Some(drag) = self.drag_state.as_mut() {
            drag.current_x = x;
            drag.current_y = y;
            if !drag.active && (x - drag.start_x).abs() + (y - drag.start_y).abs() > 6 {
                drag.active = true;
                unsafe {
                    let _ = SetCapture(self.hwnd);
                }
                self.create_drag_ghost();
            }
        }

        let old_close = self.hover_close;
        let old_tab = self.hover_tab;
        let old_folder = self.hover_folder;
        let old_target = self.hover_target;
        let old_mode_menu = self.mode_menu_open;
        let old_hovering = self.hovering_sidebar;
        self.hover_close = None;
        self.hover_tab = None;
        self.hover_folder = None;
        self.hover_target = None;
        self.drop_target = Some(DropTarget::None);

        if x < HOVER_ZONE && self.sidebar_mode == SidebarMode::Hidden && !self.animating_sidebar {
            self.sidebar_expand_mode = SidebarMode::Overlay;
            self.set_sidebar_mode(SidebarMode::Overlay);
        }

        let in_sidebar_hover_zone = if self.sidebar_width > 0.5 {
            (x as f32) < self.sidebar_width + 4.0
        } else {
            x < HOVER_ZONE
        };
        self.hovering_sidebar = in_sidebar_hover_zone;

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
            } else if point_in_rect(x, y, self.address_pill_rect()) {
                self.hover_target = Some(HoverTarget::Address);
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

        if let Some(SidebarHit::Tab(tab_array_index)) = self.hit_sidebar(x, y) {
            self.hover_tab = Some(tab_array_index);
            for (_, rect) in self.sidebar_row_rects() {
                if point_in_rect(x, y, rect) && x >= rect.right - 34 {
                    self.hover_close = Some(tab_array_index);
                    break;
                }
            }
        } else {
            self.hover_tab = None;
            self.hover_close = None;
        }
        if let Some(SidebarHit::Folder(_)) = self.hit_sidebar(x, y) {
            self.hover_folder = self.hit_sidebar(x, y).and_then(|h| {
                if let SidebarHit::Folder(id) = h {
                    Some(id)
                } else {
                    None
                }
            });
        } else {
            self.hover_folder = None;
        }

        if self.drag_state.as_ref().map(|d| d.active).unwrap_or(false) {
            self.drop_target = Some(self.calculate_drop_target(x, y));
        }

        if !self.animating_sidebar
            && (old_close != self.hover_close
                || old_tab != self.hover_tab
                || old_folder != self.hover_folder
                || old_target != self.hover_target
                || old_mode_menu != self.mode_menu_open
                || old_hovering != self.hovering_sidebar)
        {
            self.refresh();
        }
    }

    fn start_drag_candidate(&mut self, x: i32, y: i32) {
        if self.renaming_folder_id.is_some() {
            return;
        }
        if let Some(SidebarHit::Tab(index)) = self.hit_sidebar(x, y) {
            if let Some((_, row)) = self
                .sidebar_row_rects()
                .into_iter()
                .find(|(row, _)| matches!(row, SidebarRow::Tab(row_index) if *row_index == index))
            {
                let audio_hit = self
                    .tabs
                    .get(index)
                    .map(|tab| tab.audio_playing || tab.muted)
                    .unwrap_or(false)
                    && point_in_rect(x, y, self.tab_audio_rect(row));
                if x < row.right - 34 && !audio_hit {
                    self.drag_state = Some(DragState {
                        source: DragSource::Tab(index),
                        start_x: x,
                        start_y: y,
                        active: false,
                        current_x: x,
                        current_y: y,
                    });
                }
            }
        } else if let Some(SidebarHit::Folder(folder_id)) = self.hit_sidebar(x, y) {
            if let Some((_, _row)) = self
                .sidebar_row_rects()
                .into_iter()
                .find(|(row, _)| matches!(row, SidebarRow::Folder(id) if *id == folder_id))
            {
                self.drag_state = Some(DragState {
                    source: DragSource::Folder(folder_id),
                    start_x: x,
                    start_y: y,
                    active: false,
                    current_x: x,
                    current_y: y,
                });
            }
        }
    }

    fn finish_drag(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = self.drag_state.take() else {
            return false;
        };
        unsafe {
            let _ = ReleaseCapture();
        }
        *self.drag_ghost.borrow_mut() = None;
        self.drop_target = Some(DropTarget::None);
        if !drag.active {
            return false;
        }
        self.handle_drop(drag.source, x, y);
        true
    }

    fn handle_drop(&mut self, source: DragSource, x: i32, y: i32) {
        let hit = self.hit_sidebar(x, y);
        match source {
            DragSource::Tab(from_index) => {
                if from_index >= self.tabs.len() {
                    return;
                }
                let dragged_pinned = self.tabs[from_index].pinned;
                let dragged_workspace = self.tabs[from_index].workspace_id;

                match hit {
                    Some(SidebarHit::PinnedSection) | Some(SidebarHit::WorkspaceHeader) => {
                        if let Some(tab) = self.tabs.get_mut(from_index) {
                            tab.pinned = true;
                            tab.folder_id = None;
                        }
                    }
                    Some(SidebarHit::Folder(folder_id)) => {
                        if let Some(folder) = self.folders.iter().find(|folder| {
                            folder.id == folder_id && folder.workspace_id == dragged_workspace && !folder.pinned
                        }) {
                            if !dragged_pinned {
                                let _ = folder;
                                if let Some(tab) = self.tabs.get_mut(from_index) {
                                    tab.folder_id = Some(folder_id);
                                }
                            }
                        }
                    }
                    Some(SidebarHit::Tab(target_index)) if target_index < self.tabs.len() => {
                        if target_index == from_index {
                            return;
                        }
                        if !dragged_pinned && self.tabs[target_index].pinned {
                            return;
                        }
                        let target_id = self.tabs[target_index].id;
                        let target_folder = self.tabs[target_index].folder_id;
                        let target_pinned = self.tabs[target_index].pinned;
                        let tab_id = self.tabs[from_index].id;
                        let mut tab = self.tabs.remove(from_index);
                        tab.pinned = if dragged_pinned { target_pinned } else { false };
                        tab.folder_id = if tab.pinned { None } else { target_folder };
                        let insert_at = self
                            .tabs
                            .iter()
                            .position(|candidate| candidate.id == target_id)
                            .unwrap_or_else(|| target_index.min(self.tabs.len()));
                        self.tabs.insert(insert_at, tab);
                        if let Some(new_active) = self.tabs.iter().position(|tab| tab.id == tab_id) {
                            self.active = new_active;
                        }
                    }
                    _ => {}
                }
            }
            DragSource::Folder(from_folder_id) => {
                let from_workspace = self
                    .folders
                    .iter()
                    .find(|f| f.id == from_folder_id)
                    .map(|f| f.workspace_id);

                match hit {
                    Some(SidebarHit::PinnedSection) | Some(SidebarHit::WorkspaceHeader) => {
                        if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                            folder.pinned = true;
                        }
                    }
                    Some(SidebarHit::Folder(target_folder_id)) => {
                        if target_folder_id == from_folder_id {
                            return;
                        }
                        let target_pinned = self
                            .folders
                            .iter()
                            .find(|f| f.id == target_folder_id)
                            .map(|f| f.pinned);
                        if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                            if let Some(target_pinned) = target_pinned {
                                folder.pinned = target_pinned;
                            }
                        }
                        if let (Some(workspace_id), Some(target_pinned)) = (from_workspace, target_pinned) {
                            let folder_indices: Vec<usize> = self
                                .folders
                                .iter()
                                .enumerate()
                                .filter(|(_, f)| f.workspace_id == workspace_id && f.pinned == target_pinned)
                                .map(|(i, _)| i)
                                .collect();
                            let from_idx_pos = folder_indices.iter().position(|&i| {
                                self.folders[i].id == from_folder_id
                            });
                            let target_idx_pos = folder_indices.iter().position(|&i| {
                                self.folders[i].id == target_folder_id
                            });
                            if let (Some(from_idx_pos), Some(target_idx_pos)) = (from_idx_pos, target_idx_pos) {
                                let from_global_idx = folder_indices[from_idx_pos];
                                let folder_to_move = self.folders.remove(from_global_idx);
                                let new_indices: Vec<usize> = self
                                    .folders
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, f)| f.workspace_id == workspace_id && f.pinned == target_pinned)
                                    .map(|(i, _)| i)
                                    .collect();
                                let insert_pos = if from_idx_pos < target_idx_pos {
                                    target_idx_pos - 1
                                } else {
                                    target_idx_pos
                                };
                                let global_insert = new_indices.get(insert_pos)
                                    .copied()
                                    .unwrap_or(self.folders.len());
                                self.folders.insert(global_insert, folder_to_move);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.save_state();
        self.refresh();
    }

    fn calculate_drop_target(&self, x: i32, y: i32) -> DropTarget {
        let hit = self.hit_sidebar(x, y);
        match hit {
            Some(SidebarHit::PinnedSection) => DropTarget::PinnedSection,
            Some(SidebarHit::Folder(folder_id)) => DropTarget::Folder(folder_id),
            Some(SidebarHit::Tab(index)) => DropTarget::Tab(index),
            _ => DropTarget::None,
        }
    }

    fn create_drag_ghost(&self) {
        let Some(drag) = self.drag_state else {
            return;
        };
        let width = self.sidebar_width();
        if width <= 92 {
            return;
        }

        let (ghost_width, ghost_height) = match drag.source {
            DragSource::Tab(_) => ((width - 20) as i32, 44),
            DragSource::Folder(_) => ((width - 20) as i32, 36),
        };

        unsafe {
            let hdc_screen = Gdi::GetDC(None);
            let mem_dc = CreateCompatibleDC(Some(hdc_screen));
            let bitmap = CreateCompatibleBitmap(hdc_screen, ghost_width, ghost_height);
            let old = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
            let black = CreateSolidBrush(COLORREF(0x000000));
            let _ = FillRect(
                mem_dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: ghost_width,
                    bottom: ghost_height,
                },
                black,
            );
            let _ = DeleteObject(HGDIOBJ(black.0));

            let item = RECT {
                left: 0,
                top: 0,
                right: ghost_width,
                bottom: ghost_height,
            };
            fill_round_rect(mem_dc, item, 0x1a1a1a, 10);

            match drag.source {
                DragSource::Tab(index) => {
                    if let Some(tab) = self.tabs.get(index) {
                        if tab.pinned {
                            draw_icon_glyph(
                                mem_dc,
                                &self.fonts.toolbar_icon,
                                glyph(0xE718).as_str(),
                                RECT {
                                    left: 8,
                                    top: 0,
                                    right: 28,
                                    bottom: ghost_height,
                                },
                                COLOR_ACCENT,
                            );
                        }
                        let favicon_left = if tab.pinned { 34 } else { 12 };
                        let favicon = RECT {
                            left: favicon_left,
                            top: 11,
                            right: favicon_left + 18,
                            bottom: 29,
                        };
                        draw_tab_favicon(mem_dc, &self.fonts.small, favicon, tab);
                        draw_text(
                            mem_dc,
                            &self.fonts.body,
                            &tab.title,
                            RECT {
                                left: if tab.pinned { 62 } else { 40 },
                                top: 0,
                                right: ghost_width - 8,
                                bottom: ghost_height,
                            },
                            COLOR_TEXT,
                        );
                    }
                }
                DragSource::Folder(folder_id) => {
                    if let Some(folder) = self.folders.iter().find(|f| f.id == folder_id) {
                        let folder_arrow = if folder.pinned {
                            glyph(0xE718)
                        } else {
                            glyph(0xE8B7)
                        };
                        draw_icon_glyph(
                            mem_dc,
                            &self.fonts.toolbar_icon,
                            folder_arrow.as_str(),
                            RECT {
                                left: 8,
                                top: 0,
                                right: 30,
                                bottom: ghost_height,
                            },
                            if folder.pinned { COLOR_ACCENT } else { COLOR_MUTED },
                        );
                        draw_text(
                            mem_dc,
                            &self.fonts.body,
                            &folder.name,
                            RECT {
                                left: 36,
                                top: 0,
                                right: ghost_width - 8,
                                bottom: ghost_height,
                            },
                            COLOR_MUTED,
                        );
                    }
                }
            }

            SelectObject(mem_dc, old);
            let _ = DeleteDC(mem_dc);
            Gdi::ReleaseDC(None, hdc_screen);

            *self.drag_ghost.borrow_mut() = Some(DragGhost {
                handle: bitmap,
                width: ghost_width,
                height: ghost_height,
            });
        }
    }

    fn toggle_sidebar(&mut self) {
        if self.sidebar_mode == SidebarMode::Pushed || self.sidebar_expand_mode == SidebarMode::Pushed {
            self.set_sidebar_mode(SidebarMode::Hidden);
        } else if self.sidebar_mode == SidebarMode::Overlay || self.sidebar_expand_mode == SidebarMode::Overlay {
            self.sidebar_expand_mode = SidebarMode::Pushed;
            if !self.animating_sidebar {
                self.sidebar_mode = SidebarMode::Pushed;
            }
            self.layout();
            self.refresh();
        } else {
            self.sidebar_expand_mode = SidebarMode::Pushed;
            self.set_sidebar_mode(SidebarMode::Pushed);
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
            if mode != SidebarMode::Hidden {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
            }
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
                self.sidebar_expand_mode = SidebarMode::Hidden;
                self.hovering_sidebar = false;
                self.clear_webview_clipping();
                self.ensure_hover_detect_timer();
            } else if self.sidebar_target >= SIDEBAR_EXPANDED {
                self.sidebar_mode = self.sidebar_expand_mode;
                if self.sidebar_mode == SidebarMode::Overlay {
                    self.clear_webview_clipping();
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
            self.layout();
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
                let _ = Gdi::UpdateWindow(self.hwnd);
            }
        } else {
            self.sidebar_width += distance * 0.22;
            self.layout();
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    fn clear_webview_clipping(&self) {
        self.last_clip_width.set(0.0);
        for tab in &self.tabs {
            unsafe {
                let _ = SetWindowRgn(tab.child_hwnd, None, false);
            }
        }
    }

    fn check_hover_leave(&mut self) {
        if self.sidebar_mode != SidebarMode::Overlay {
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
            }
            return;
        }
        if self.animating_sidebar {
            return;
        }
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                if ScreenToClient(self.hwnd, &mut pt).as_bool() {
                    let sidebar_w = self.sidebar_width() as i32;
                    if pt.x > sidebar_w + HOVER_ZONE || pt.x < 0 || pt.y < 0 || pt.y > 10000 {
                        let _ =
                            WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
                        self.sidebar_expand_mode = SidebarMode::Hidden;
                        self.set_sidebar_mode(SidebarMode::Hidden);
                    }
                }
            }
        }
    }

    fn check_hover_detect(&mut self) {
        if self.sidebar_mode != SidebarMode::Hidden || self.animating_sidebar {
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
            }
            return;
        }
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                if ScreenToClient(self.hwnd, &mut pt).as_bool() {
                    if pt.x < HOVER_ZONE && pt.x >= 0 && pt.y >= 0 {
                        let _ =
                            WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
                        self.sidebar_expand_mode = SidebarMode::Overlay;
                        self.hovering_sidebar = true;
                        self.set_sidebar_mode(SidebarMode::Overlay);
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
            style: WindowsAndMessaging::WNDCLASS_STYLES(0),
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
        if let Some(icon) = create_blank_icon(16) {
            let _ = WindowsAndMessaging::SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
        if let Some(icon) = create_aster_icon(64) {
            let _ = WindowsAndMessaging::SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
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

fn create_command_popup(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_CLIPSIBLINGS.0),
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(COMMAND_POPUP_ID as usize as *mut _)),
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        OLD_COMMAND_POPUP_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            command_popup_proc as *const () as isize,
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
    if msg == WM_KEYDOWN && w_param.0 as u32 == VK_ESCAPE.0 as u32 {
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| app.close_command());
        }
        return LRESULT(0);
    }
    if msg == WM_CHAR && w_param.0 as u32 == VK_RETURN.0 as u32 {
        return LRESULT(0);
    }
    WindowsAndMessaging::CallWindowProcW(OLD_ADDRESS_PROC, hwnd, msg, w_param, l_param)
}

unsafe extern "system" fn command_popup_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| app.paint_command_popup(hdc));
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONDOWN => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| {
                    for (row_index, (tab_index, _title, url)) in
                        app.command_suggestions().into_iter().take(5).enumerate()
                    {
                        let mut row = app.command_tab_row_rect(row_index);
                        let popup = app.command_popup_rect();
                        row.left -= popup.left;
                        row.right -= popup.left;
                        row.top -= popup.top;
                        row.bottom -= popup.top;
                        if point_in_rect(x, y, row) {
                            app.close_command();
                            if let Some(tab_index) = tab_index {
                                app.switch_to(tab_index);
                            } else {
                                match app.command_mode {
                                    CommandMode::NewTab => {
                                        let _ = app.create_tab(&url);
                                    }
                                    _ => app.navigate_active(&url),
                                }
                            }
                            return;
                        }
                    }
                    let _ = SetFocus(Some(app.address_hwnd));
                });
            }
            LRESULT(0)
        }
        _ => WindowsAndMessaging::CallWindowProcW(
            OLD_COMMAND_POPUP_PROC,
            hwnd,
            msg,
            w_param,
            l_param,
        ),
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let _ = l_param.0 as *const CREATESTRUCTW;
            LRESULT(1)
        }
        WM_CREATE => LRESULT(0),
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            with_app(hwnd, |app| {
                app.layout();
                app.refresh();
            });
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
                let _ = FillRect(
                    mem_dc,
                    &rect,
                    with_app_return(hwnd, |app| app.brushes.black).unwrap_or(solid_brush(0)),
                );
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
            with_app(hwnd, |app| {
                app.start_drag_candidate(x, y);
                app.handle_click(x, y);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| {
                let _ = app.finish_drag(x, y);
            });
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| app.handle_right_click(x, y));
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| app.handle_mouse_move(x, y));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(w_param.0 as u32) as i16 as i32;
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            let mut pt = POINT { x, y };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            with_app(hwnd, |app| {
                if (pt.x as f32) < app.sidebar_width && app.sidebar_width() > 92 {
                    app.switch_workspace_by_delta(if delta < 0 { 1 } else { -1 });
                }
            });
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
            if w_param.0 == HOVER_DETECT_TIMER_ID {
                with_app(hwnd, |app| app.check_hover_detect());
                return LRESULT(0);
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_COMMAND => {
            let id = loword(w_param.0 as u32) as i32;
            let code = hiword(w_param.0 as u32) as u16;
            if id == ADDRESS_ID {
                if code == 0x0300 {
                    with_app(hwnd, |app| {
                        if app.command_open {
                            unsafe {
                                let _ = InvalidateRect(Some(app.command_hwnd), None, false);
                            }
                        }
                    });
                }
                return LRESULT(0);
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_CHAR => {
            let mut handled = false;
            with_app(hwnd, |app| {
                if app.renaming_folder_id.is_some() {
                    let ch = w_param.0 as u32;
                    if ch == 13 {
                        app.confirm_rename();
                        handled = true;
                    } else if ch == 27 {
                        app.cancel_rename();
                        handled = true;
                    } else if ch == 8 {
                        app.rename_buffer.pop();
                        app.refresh();
                        handled = true;
                    } else if ch >= 32 && ch < 127 {
                        app.rename_buffer.push(ch as u8 as char);
                        app.refresh();
                        handled = true;
                    }
                }
            });
            if handled {
                return LRESULT(0);
            }
            if w_param.0 as u32 == VK_RETURN.0 as u32 {
                with_app(hwnd, |app| app.navigate_active_from_address());
                return LRESULT(0);
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_KEYDOWN => {
            let mut handled = false;
            with_app(hwnd, |app| {
                if app.renaming_folder_id.is_some() {
                    let key = w_param.0 as u32;
                    if key == 13 {
                        app.confirm_rename();
                        handled = true;
                    } else if key == 27 {
                        app.cancel_rename();
                        handled = true;
                    }
                }
            });
            if handled {
                return LRESULT(0);
            }
            handle_keydown(hwnd, w_param);
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_SETFOCUS => {
            with_app(hwnd, |app| {
                if let Some(tab) = app.active_tab_index().and_then(|index| app.tabs.get(index)) {
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
                app.open_command(CommandMode::Navigate);
            }
            0x54 if ctrl => {
                app.open_command(CommandMode::NewTab);
            }
            0x53 if ctrl => app.toggle_sidebar(),
            0x57 if ctrl => {
                if let Some(index) = app.active_tab_index() {
                    app.close_tab(index);
                }
            }
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

fn draw_toolbar_icon_button(
    hdc: HDC,
    rect: RECT,
    icon: IconKind,
    _hovered: bool,
    icon_font: &HFONT,
) {
    unsafe {
        draw_icon_glyph(
            hdc,
            icon_font,
            icon.glyph(),
            RECT {
                left: rect.left + 1,
                top: rect.top + 1,
                right: rect.right - 1,
                bottom: rect.bottom - 1,
            },
            COLOR_TEXT,
        );
    }
}

fn draw_logo(hdc: HDC, rect: RECT, hovered: bool) {
    unsafe {
        let color = if hovered {
            mix_color(COLOR_ACCENT, COLOR_TEXT, 0.22)
        } else {
            COLOR_ACCENT
        };
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

unsafe fn draw_centered_text(hdc: HDC, font: &HFONT, text: &str, mut rect: RECT, color: u32) {
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, COLORREF(color));
    let mut wide = to_wide(text);
    let text_len = wide.len().saturating_sub(1);
    let _ = DrawTextW(
        hdc,
        &mut wide[..text_len],
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
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

unsafe fn draw_tab_favicon(hdc: HDC, font: &HFONT, rect: RECT, tab: &Tab) {
    if let Some(favicon) = tab.favicon_bitmap.as_ref() {
        draw_bitmap_fit(hdc, rect, favicon);
        return;
    }
    let host = display_host(&tab.url);
    let label_source = if host.is_empty() {
        tab.title.as_str()
    } else {
        host.as_str()
    };
    let letter = label_source
        .chars()
        .find(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase().to_string())
        .unwrap_or_else(|| "A".to_string());
    draw_centered_text(hdc, font, &letter, rect, COLOR_ACCENT);
}

unsafe fn draw_bitmap_fit(hdc: HDC, rect: RECT, bitmap: &FaviconBitmap) {
    let target_w = rect.right - rect.left;
    let target_h = rect.bottom - rect.top;
    if target_w <= 0 || target_h <= 0 || bitmap.width <= 0 || bitmap.height <= 0 {
        return;
    }
    let scale = (target_w as f32 / bitmap.width as f32).min(target_h as f32 / bitmap.height as f32);
    let width = (bitmap.width as f32 * scale).round() as i32;
    let height = (bitmap.height as f32 * scale).round() as i32;
    let x = rect.left + (target_w - width) / 2;
    let y = rect.top + (target_h - height) / 2;
    let mem_dc = CreateCompatibleDC(Some(hdc));
    if mem_dc.is_invalid() {
        return;
    }
    let old = SelectObject(mem_dc, HGDIOBJ(bitmap.handle.0));
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = AlphaBlend(
        hdc,
        x,
        y,
        width,
        height,
        mem_dc,
        0,
        0,
        bitmap.width,
        bitmap.height,
        blend,
    );
    let _ = SelectObject(mem_dc, old);
    let _ = DeleteDC(mem_dc);
}

fn decode_favicon_stream(stream: &IStream) -> Option<FaviconBitmap> {
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(stream, ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None::<&windows::Win32::Graphics::Imaging::IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut width = 0u32;
        let mut height = 0u32;
        converter.GetSize(&mut width, &mut height).ok()?;
        if width == 0 || height == 0 || width > 256 || height > 256 {
            return None;
        }
        let stride = width * 4;
        let mut pixels = vec![0u8; (stride * height) as usize];
        converter
            .CopyPixels(ptr::null(), stride, &mut pixels)
            .ok()?;
        let handle = create_bgra_bitmap(width as i32, height as i32, &pixels)?;
        Some(FaviconBitmap {
            handle,
            width: width as i32,
            height: height as i32,
        })
    }
}

fn create_aster_icon(size: i32) -> Option<HICON> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let radius = size as f32 * 0.30;
    let stroke = (size as f32 * 0.13).max(4.0);
    draw_icon_line(&mut pixels, size, cx, cy - radius, cx, cy + radius, stroke);
    draw_icon_line(
        &mut pixels,
        size,
        cx - radius * 0.9,
        cy - radius * 0.5,
        cx + radius * 0.9,
        cy + radius * 0.5,
        stroke,
    );
    draw_icon_line(
        &mut pixels,
        size,
        cx + radius * 0.9,
        cy - radius * 0.5,
        cx - radius * 0.9,
        cy + radius * 0.5,
        stroke,
    );
    unsafe {
        let color = create_bgra_bitmap(size, size, &pixels)?;
        let mask = CreateBitmap(size, size, 1, 1, None);
        if mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(color.0));
            return None;
        }
        let info = ICONINFO {
            fIcon: BOOL::from(true),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

fn create_blank_icon(size: i32) -> Option<HICON> {
    let pixels = vec![0u8; (size * size * 4) as usize];
    unsafe {
        let color = create_bgra_bitmap(size, size, &pixels)?;
        let mask = CreateBitmap(size, size, 1, 1, None);
        if mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(color.0));
            return None;
        }
        let info = ICONINFO {
            fIcon: BOOL::from(true),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

fn draw_icon_line(pixels: &mut [u8], size: i32, x1: f32, y1: f32, x2: f32, y2: f32, stroke: f32) {
    let steps = ((x2 - x1).abs().max((y2 - y1).abs()) * 2.0).max(1.0) as i32;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = x1 + (x2 - x1) * t;
        let y = y1 + (y2 - y1) * t;
        draw_icon_dot(pixels, size, x, y, stroke / 2.0);
    }
}

fn draw_icon_dot(pixels: &mut [u8], size: i32, cx: f32, cy: f32, radius: f32) {
    let min_x = (cx - radius).floor() as i32;
    let max_x = (cx + radius).ceil() as i32;
    let min_y = (cy - radius).floor() as i32;
    let max_y = (cy + radius).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if x < 0 || y < 0 || x >= size || y >= size {
                continue;
            }
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= radius * radius {
                let index = ((y * size + x) * 4) as usize;
                pixels[index] = 0xf1;
                pixels[index + 1] = 0x6f;
                pixels[index + 2] = 0x63;
                pixels[index + 3] = 0xff;
            }
        }
    }
}

unsafe fn create_bgra_bitmap(width: i32, height: i32, pixels: &[u8]) -> Option<HBITMAP> {
    if width <= 0 || height <= 0 || pixels.len() < (width * height * 4) as usize {
        return None;
    }
    let mut info = BITMAPINFO::default();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = ptr::null_mut();
    let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    if bits.is_null() {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        return None;
    }
    ptr::copy_nonoverlapping(
        pixels.as_ptr(),
        bits as *mut u8,
        (width * height * 4) as usize,
    );
    Some(bitmap)
}

fn render_aster_background_bitmap(width: i32, height: i32) -> Option<BackgroundBitmap> {
    let _ = ASTER_BACKGROUND_SVG.len();
    let width = width.max(1);
    let height = height.max(1);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let accent = (0x63u8, 0x6fu8, 0xf1u8);
    let sx = width as f32 / 1920.0;
    let sy = height as f32 / 1080.0;
    let scale = sx.min(sy).max(0.35);

    for y in 0..height {
        let ny = y as f32 / height as f32;
        for x in 0..width {
            let nx = x as f32 / width as f32;
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;

            let dm = ((nx * nx) + ((ny - 1.0) * (ny - 1.0))).sqrt() / 1.10;
            let gm = if dm <= 0.45 {
                0.60 + (0.15 - 0.60) * (dm / 0.45)
            } else if dm <= 1.0 {
                0.15 * (1.0 - (dm - 0.45) / 0.55)
            } else {
                0.0
            };
            blend_rgb(&mut r, &mut g, &mut b, accent, gm);

            let ds = (((nx - 1.0) * (nx - 1.0)) + (ny * ny)).sqrt() / 0.65;
            let gs = if ds <= 1.0 { 0.22 * (1.0 - ds) } else { 0.0 };
            blend_rgb(&mut r, &mut g, &mut b, accent, gs);

            let dc = (((nx - 0.5) * (nx - 0.5)) + ((ny - 0.5) * (ny - 0.5))).sqrt() / 0.50;
            let gc = if dc <= 1.0 { 0.06 * (1.0 - dc) } else { 0.0 };
            blend_rgb(&mut r, &mut g, &mut b, accent, gc);

            let d_bottom = ((x as f32).powi(2) + (y as f32 - height as f32).powi(2)).sqrt();
            for (radius, alpha, stroke) in [
                (280.0f32, 0.12f32, 1.0f32),
                (460.0f32, 0.08f32, 0.8f32),
                (650.0f32, 0.05f32, 0.7f32),
                (880.0f32, 0.03f32, 0.6f32),
                (1100.0f32, 0.02f32, 0.5f32),
            ] {
                if (d_bottom - radius * scale).abs() <= stroke.max(0.7f32) {
                    blend_rgb(&mut r, &mut g, &mut b, accent, alpha);
                }
            }

            let d_top = ((x as f32 - width as f32).powi(2) + (y as f32).powi(2)).sqrt();
            for (radius, alpha, stroke) in
                [(220.0f32, 0.07f32, 0.7f32), (400.0f32, 0.04f32, 0.5f32)]
            {
                if (d_top - radius * scale).abs() <= stroke.max(0.7f32) {
                    blend_rgb(&mut r, &mut g, &mut b, accent, alpha);
                }
            }

            let index = ((y * width + x) * 4) as usize;
            pixels[index] = b.round().clamp(0.0, 255.0) as u8;
            pixels[index + 1] = g.round().clamp(0.0, 255.0) as u8;
            pixels[index + 2] = r.round().clamp(0.0, 255.0) as u8;
            pixels[index + 3] = 255;
        }
    }

    let dot_step = (48.0 * scale).round().max(28.0) as i32;
    let dot_radius = (0.9 * scale).max(0.7);
    let mut y = (24.0 * scale) as i32;
    while y < height {
        let mut x = (24.0 * scale) as i32;
        while x < width {
            blend_disc(
                &mut pixels,
                width,
                height,
                x as f32,
                y as f32,
                dot_radius,
                (255, 255, 255),
                0.18,
            );
            x += dot_step;
        }
        y += dot_step;
    }

    for (x, y, radius, amount, white) in [
        (192.0, 216.0, 1.8, 0.60, false),
        (576.0, 108.0, 1.2, 0.40, true),
        (960.0, 270.0, 1.5, 0.40, false),
        (1440.0, 162.0, 2.0, 0.35, true),
        (1700.0, 350.0, 1.2, 0.45, false),
        (1300.0, 500.0, 1.6, 0.25, true),
        (340.0, 480.0, 1.4, 0.50, false),
        (740.0, 670.0, 1.1, 0.30, true),
        (1580.0, 770.0, 1.7, 0.30, false),
        (230.0, 830.0, 1.9, 0.55, false),
        (1190.0, 920.0, 1.2, 0.20, true),
        (1780.0, 990.0, 1.4, 0.25, false),
        (620.0, 900.0, 1.0, 0.30, true),
        (860.0, 80.0, 1.3, 0.35, false),
        (1060.0, 760.0, 1.1, 0.30, false),
        (440.0, 200.0, 0.9, 0.25, true),
        (1820.0, 540.0, 1.5, 0.20, false),
        (90.0, 680.0, 1.6, 0.40, false),
    ] {
        let color = if white { (255, 255, 255) } else { accent };
        blend_disc(
            &mut pixels,
            width,
            height,
            x * sx,
            y * sy,
            (radius * scale).max(0.8),
            color,
            amount,
        );
    }

    unsafe { create_bgra_bitmap(width, height, &pixels) }.map(|handle| BackgroundBitmap {
        handle,
        width,
        height,
    })
}

fn blend_rgb(r: &mut f32, g: &mut f32, b: &mut f32, color: (u8, u8, u8), alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    *r = *r * (1.0 - a) + color.0 as f32 * a;
    *g = *g * (1.0 - a) + color.1 as f32 * a;
    *b = *b * (1.0 - a) + color.2 as f32 * a;
}

fn blend_disc(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    color: (u8, u8, u8),
    alpha: f32,
) {
    let min_x = (cx - radius).floor() as i32;
    let max_x = (cx + radius).ceil() as i32;
    let min_y = (cy - radius).floor() as i32;
    let max_y = (cy + radius).ceil() as i32;
    let radius_sq = radius * radius;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if x < 0 || y < 0 || x >= width || y >= height {
                continue;
            }
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy > radius_sq {
                continue;
            }
            let index = ((y * width + x) * 4) as usize;
            let a = alpha.clamp(0.0, 1.0);
            pixels[index] = (pixels[index] as f32 * (1.0 - a) + color.2 as f32 * a)
                .round()
                .clamp(0.0, 255.0) as u8;
            pixels[index + 1] = (pixels[index + 1] as f32 * (1.0 - a) + color.1 as f32 * a)
                .round()
                .clamp(0.0, 255.0) as u8;
            pixels[index + 2] = (pixels[index + 2] as f32 * (1.0 - a) + color.0 as f32 * a)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
}

fn glyph(codepoint: u32) -> String {
    char::from_u32(codepoint).unwrap_or(' ').to_string()
}

fn menu_item(id: usize, label: &str) -> OverlayMenuItem {
    OverlayMenuItem {
        id,
        label: label.to_string(),
        sublabel: String::new(),
    }
}

fn menu_item_with_subtitle(id: usize, label: &str, sublabel: &str) -> OverlayMenuItem {
    OverlayMenuItem {
        id,
        label: label.to_string(),
        sublabel: sublabel.to_string(),
    }
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

fn collect_direct_children(parent: HWND) -> Vec<HWND> {
    let mut children = Vec::new();
    unsafe {
        let mut child = GetTopWindow(Some(parent)).unwrap_or_default();
        while !child.is_invalid() {
            children.push(child);
            child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
        }
    }
    children
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

fn set_edit_cue_banner(hwnd: HWND, text: &str) {
    const EM_SETCUEBANNER: u32 = 0x1501;
    let wide = to_wide(text);
    unsafe {
        let _ = WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_SETCUEBANNER,
            Some(WPARAM(0)),
            Some(LPARAM(wide.as_ptr() as isize)),
        );
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

fn display_host(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme
        .trim_start_matches("www.")
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

fn state_path() -> PathBuf {
    PathBuf::from(STATE_FILE)
}

fn serialize_history(entries: &[HistoryEntry]) -> String {
    entries
        .iter()
        .filter(|entry| !entry.url.trim().is_empty())
        .map(|entry| {
            format!(
                "{}\u{1f}{}",
                escape_state(&entry.title),
                escape_state(&entry.url)
            )
        })
        .collect::<Vec<_>>()
        .join("\u{1e}")
}

fn parse_history(raw: &str) -> Vec<HistoryEntry> {
    raw.split('\u{1e}')
        .filter_map(|entry| {
            let mut parts = entry.splitn(2, '\u{1f}');
            let title = parts.next().unwrap_or_default().to_string();
            let url = parts.next().unwrap_or_default().to_string();
            if url.trim().is_empty() {
                None
            } else {
                Some(HistoryEntry { title, url })
            }
        })
        .collect()
}

fn escape_state(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\u{1e}', "\\r")
        .replace('\u{1f}', "\\u")
}

fn unescape_state(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\u{1e}'),
                Some('u') => out.push('\u{1f}'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
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
