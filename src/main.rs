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
            DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_CAPTION_COLOR,
            DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
        },
        Graphics::Gdi::{
            self, AlphaBlend, BeginPaint, BitBlt, CreateBitmap, CreateCompatibleBitmap,
            CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateRectRgn,
            CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
            GetMonitorInfoW, GetStockObject, InvalidateRect, LineTo, MonitorFromWindow, MoveToEx,
            RoundRect, ScreenToClient, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
            StretchBlt,
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
            Controls::{EM_SETMARGINS, EM_SETSEL, MARGINS},
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
                WNDPROC, WS_CHILD, WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW,
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
const BACKGROUND_TIMER_ID: usize = 45;
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
const MENU_TAB_DELETE_PIN: usize = 3507;
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
#[allow(dead_code)]
const COLOR_SELECTION: u32 = 0xf16f63; // Signature Accent Color (#636ff1)
const ASTER_BACKGROUND_SVG: &str = include_str!("../assets/aster-background.svg");

static mut OLD_ADDRESS_PROC: WNDPROC = None;
static mut OLD_COMMAND_POPUP_PROC: WNDPROC = None;
static mut OLD_RENAME_EDIT_PROC: WNDPROC = None;
static mut OLD_OVERLAY_MENU_PROC: WNDPROC = None;
static mut OLD_DRAG_GHOST_PROC: WNDPROC = None;
static mut CURRENT_DRAG_GHOST_BITMAP: Option<HBITMAP> = None;

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
    parent_id: Option<usize>,
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
    unloaded: bool,
}

#[derive(Clone)]
struct HistoryEntry {
    title: String,
    url: String,
}

#[derive(Clone, Debug)]
struct VisitedSite {
    url: String,
    visit_count: u32,
    last_visit_time: u64,
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn calculate_frecency(visit_count: u32, last_visit_time: u64, current_time: u64) -> u32 {
    let age_seconds = current_time.saturating_sub(last_visit_time);
    let age_hours = age_seconds / 3600;
    
    let recency_weight = if age_hours < 4 {
        100
    } else if age_hours < 24 {
        80
    } else if age_hours < 24 * 7 {
        60
    } else if age_hours < 24 * 30 {
        30
    } else {
        10
    };
    
    visit_count * recency_weight
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

#[allow(dead_code)]
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
    MinButton,
    MaxButton,
    CloseButton,
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
    hover: HBRUSH,
}

impl Drop for UiBrushes {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.black.0));
            let _ = DeleteObject(HGDIOBJ(self.panel.0));
            let _ = DeleteObject(HGDIOBJ(self.panel_2.0));
            let _ = DeleteObject(HGDIOBJ(self.edit.0));
            let _ = DeleteObject(HGDIOBJ(self.hover.0));
        }
    }
}

struct App {
    hwnd: HWND,
    address_hwnd: HWND,
    command_hwnd: HWND,
    overlay_menu_hwnd: HWND,
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
    visited_sites: Vec<VisitedSite>,
    command_open: bool,
    command_mode: CommandMode,
    renaming_folder_id: Option<usize>,
    rename_buffer: String,
    rename_selected: bool,
    renaming_edit: Option<HWND>,
    fullscreen: bool,
    drag_ghost_hwnd: Cell<Option<HWND>>,
    saved_style: isize,
    saved_rect: RECT,
    command_selected_index: Option<usize>,
    command_scroll_offset: usize,
    is_deleting: bool,
    last_address_text: String,
    has_typed: bool,
}

struct DragGhost {
    handle: HBITMAP,
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
            hover: solid_brush(COLOR_SURFACE_HOVER),
        };

        let address_hwnd = create_address_bar(hwnd)?;
        let command_hwnd = create_command_popup(hwnd)?;
        let overlay_menu_hwnd = create_overlay_menu(hwnd)?;
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
            overlay_menu_hwnd,
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
            visited_sites: Vec::new(),
            command_open: false,
            command_mode: CommandMode::Navigate,
            renaming_folder_id: None,
            rename_buffer: String::new(),
            rename_selected: false,
            renaming_edit: None,
            fullscreen: false,
            drag_ghost_hwnd: Cell::new(None),
            saved_style: 0,
            saved_rect: RECT::default(),
            command_selected_index: None,
            command_scroll_offset: 0,
            is_deleting: false,
            last_address_text: String::new(),
            has_typed: false,
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
            unloaded: false,
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
            self.switch_to(index, true);
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
            self.switch_to(index, false);
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

    fn folder_depth(&self, folder_id: usize) -> usize {
        let mut depth = 0;
        let mut current_id = folder_id;
        let mut visited = std::collections::HashSet::new();
        while let Some(folder) = self.folders.iter().find(|f| f.id == current_id) {
            if !visited.insert(current_id) {
                break;
            }
            if let Some(parent) = folder.parent_id {
                depth += 1;
                current_id = parent;
            } else {
                break;
            }
        }
        depth
    }

    fn tab_depth(&self, index: usize) -> usize {
        if let Some(tab) = self.tabs.get(index) {
            if let Some(folder_id) = tab.folder_id {
                return self.folder_depth(folder_id) + 1;
            }
        }
        0
    }

    fn is_descendant_of(&self, folder_id: usize, potential_ancestor_id: usize) -> bool {
        let mut current_id = folder_id;
        let mut visited = std::collections::HashSet::new();
        while let Some(folder) = self.folders.iter().find(|f| f.id == current_id) {
            if !visited.insert(current_id) {
                break;
            }
            if let Some(parent) = folder.parent_id {
                if parent == potential_ancestor_id {
                    return true;
                }
                current_id = parent;
            } else {
                break;
            }
        }
        false
    }

    fn propagate_folder_pinning(&mut self, folder_id: usize, pinned: bool) {
        for tab in self.tabs.iter_mut() {
            if tab.folder_id == Some(folder_id) {
                tab.pinned = pinned;
            }
        }
        let child_folder_ids: Vec<usize> = self.folders
            .iter()
            .filter(|f| f.parent_id == Some(folder_id))
            .map(|f| f.id)
            .collect();
        for cf_id in child_folder_ids {
            if let Some(cf) = self.folders.iter_mut().find(|f| f.id == cf_id) {
                cf.pinned = pinned;
            }
            self.propagate_folder_pinning(cf_id, pinned);
        }
    }

    fn add_folder_rows_recursive(&self, folder_id: usize, rows: &mut Vec<SidebarRow>) {
        let child_folders: Vec<&Folder> = self.folders
            .iter()
            .filter(|f| f.workspace_id == self.active_workspace && f.parent_id == Some(folder_id))
            .collect();
        for cf in child_folders {
            rows.push(SidebarRow::Folder(cf.id));
            if !cf.collapsed {
                self.add_folder_rows_recursive(cf.id, rows);
            }
        }
        let child_tabs: Vec<usize> = self.tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace
                    && tab.folder_id == Some(folder_id)
            })
            .map(|(index, _)| index)
            .collect();
        rows.extend(child_tabs.into_iter().map(SidebarRow::Tab));
    }

    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut rows = Vec::new();
        
        // Pinned Section
        let mut pinned_rows = Vec::new();
        let root_pinned_folders: Vec<&Folder> = self.folders
            .iter()
            .filter(|f| f.workspace_id == self.active_workspace && f.pinned && f.parent_id.is_none())
            .collect();
        for folder in root_pinned_folders {
            pinned_rows.push(SidebarRow::Folder(folder.id));
            if !folder.collapsed {
                self.add_folder_rows_recursive(folder.id, &mut pinned_rows);
            }
        }
        let loose_pinned_tabs: Vec<usize> = self.tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace && tab.pinned && tab.folder_id.is_none()
            })
            .map(|(index, _)| index)
            .collect();
        pinned_rows.extend(loose_pinned_tabs.into_iter().map(SidebarRow::Tab));
        rows.extend(pinned_rows);

        // Always push the divider line!
        rows.push(SidebarRow::Label(SidebarLabel::Tabs));

        // Unpinned Section
        let mut unpinned_rows = Vec::new();
        let root_unpinned_folders: Vec<&Folder> = self.folders
            .iter()
            .filter(|f| f.workspace_id == self.active_workspace && !f.pinned && f.parent_id.is_none())
            .collect();
        for folder in root_unpinned_folders {
            unpinned_rows.push(SidebarRow::Folder(folder.id));
            if !folder.collapsed {
                self.add_folder_rows_recursive(folder.id, &mut unpinned_rows);
            }
        }
        let loose_tabs: Vec<usize> = self.tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace && !tab.pinned && tab.folder_id.is_none()
            })
            .map(|(index, _)| index)
            .collect();
        unpinned_rows.extend(loose_tabs.into_iter().map(SidebarRow::Tab));
        rows.extend(unpinned_rows);

        rows
    }

    fn sidebar_row_rects(&self) -> Vec<(SidebarRow, RECT)> {
        let mut rects = Vec::new();
        let width = self.sidebar_width();
        if width <= 92 {
            return rects;
        }
        let bottom_limit = self.workspace_switcher_bounds().top - 10;
        let has_pinned = self.folders.iter().any(|f| f.workspace_id == self.active_workspace && f.pinned)
            || self.tabs.iter().any(|t| t.workspace_id == self.active_workspace && t.pinned);
        let mut y = if has_pinned {
            SIDEBAR_ROWS_TOP
        } else {
            SIDEBAR_ROWS_TOP + 72
        };
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
        let pinned_count = rows.iter().take_while(|row| {
            matches!(
                row,
                SidebarRow::Folder(_) | SidebarRow::Tab(_)
            )
        }).count();
        if pinned_count == 0 {
            let y = SIDEBAR_ROWS_TOP;
            let height = 72;
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
        self.visited_sites.clear();

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
                        let parent_id = parts.get(6)
                            .and_then(|val| if val.is_empty() { None } else { val.parse::<usize>().ok() });
                        self.folders.push(Folder {
                            id,
                            workspace_id,
                            parent_id,
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
                    let url = parts[1].clone();
                    let visit_count = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                    let last_visit_time = parts.get(3).and_then(|s| s.parse::<u64>().ok()).unwrap_or_else(|| current_timestamp());
                    self.visited_sites.push(VisitedSite {
                        url,
                        visit_count,
                        last_visit_time,
                    });
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
                    tab.unloaded = true;
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
                "folder\t{}\t{}\t{}\t{}\t{}\t{}",
                folder.id,
                folder.workspace_id,
                escape_state(&folder.name),
                if folder.collapsed { "1" } else { "0" },
                if folder.pinned { "1" } else { "0" },
                folder.parent_id.map(|id| id.to_string()).unwrap_or_default()
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
        for site in self.visited_sites.iter().take(500) {
            lines.push(format!(
                "suggestion\t{}\t{}\t{}",
                escape_state(&site.url),
                site.visit_count,
                site.last_visit_time
            ));
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
            let clean_url = strip_google_transient_params(&url);
            tab.url = if clean_url == "about:blank" {
                String::new()
            } else {
                clean_url
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
                if tab.unloaded {
                    set_window_text(self.address_hwnd, "");
                } else {
                    set_window_text(self.address_hwnd, &tab.url);
                }
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
        let now = current_timestamp();
        let norm_value = normalize_url_for_dedup(value);
        if let Some(site) = self.visited_sites.iter_mut().find(|item| normalize_url_for_dedup(&item.url) == norm_value) {
            site.visit_count += 1;
            site.last_visit_time = now;
            if value.len() < site.url.len() {
                site.url = value.to_string();
            }
        } else {
            self.visited_sites.push(VisitedSite {
                url: value.to_string(),
                visit_count: 1,
                last_visit_time: now,
            });
        }
        if self.visited_sites.len() > 500 {
            self.visited_sites.sort_by_key(|s| s.last_visit_time);
            self.visited_sites.remove(0);
        }
    }

    fn switch_to(&mut self, index: usize, wake_up: bool) {
        if index >= self.tabs.len() {
            return;
        }
        let workspace_id = self.tabs[index].workspace_id;
        self.active_workspace = workspace_id;
        if let Some(tab) = self.tabs.get_mut(index) {
            if wake_up {
                tab.unloaded = false;
            }
            if !tab.unloaded {
                unsafe {
                    let _ = WindowsAndMessaging::ShowWindow(tab.child_hwnd, WindowsAndMessaging::SW_SHOW);
                }
            } else {
                unsafe {
                    let _ = WindowsAndMessaging::ShowWindow(tab.child_hwnd, WindowsAndMessaging::SW_HIDE);
                }
            }
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            unsafe {
                let _ = tab
                    .controller
                    .SetIsVisible(i == index && tab.workspace_id == workspace_id && !tab.unloaded);
            }
        }
        self.active = index;
        self.set_workspace_active_tab(workspace_id, self.tabs[index].id);
        self.layout();
        if let Some(tab) = self.tabs.get(index) {
            if tab.unloaded {
                set_window_text(self.address_hwnd, "");
            } else {
                set_window_text(self.address_hwnd, &tab.url);
                unsafe {
                    let _ = tab
                        .controller
                        .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
                }
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

        if self.tabs[index].pinned {
            let tab = &mut self.tabs[index];
            tab.unloaded = true;
            unsafe {
                let _ = tab.controller.SetIsVisible(false);
                let _ = WindowsAndMessaging::ShowWindow(tab.child_hwnd, WindowsAndMessaging::SW_HIDE);
            }
            if self.active == index {
                let next = self.tabs
                    .iter()
                    .enumerate()
                    .find(|(i, t)| t.workspace_id == workspace_id && *i != index && !t.unloaded)
                    .map(|(i, _)| i)
                    .or_else(|| {
                        self.tabs
                            .iter()
                            .enumerate()
                            .find(|(i, t)| t.workspace_id == workspace_id && *i != index)
                            .map(|(i, _)| i)
                    });
                if let Some(next) = next {
                    self.switch_to(next, false);
                } else {
                    set_window_text(self.address_hwnd, "");
                    self.save_state();
                    self.refresh();
                    self.ensure_hover_detect_timer();
                }
            } else {
                self.save_state();
                self.refresh();
                self.ensure_hover_detect_timer();
            }
            return;
        }

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
                self.switch_to(next, false);
            }
        } else {
            self.save_state();
            self.refresh();
            self.ensure_hover_detect_timer();
        }
    }

    fn delete_pin(&mut self, index: usize) {
        if self.tabs.is_empty() || index >= self.tabs.len() {
            return;
        }
        self.tabs[index].pinned = false;
        self.close_tab(index);
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
        self.command_selected_index = None;
        self.command_scroll_offset = 0;
        self.is_deleting = false;
        self.has_typed = false;
        let initial_text = match mode {
            CommandMode::Navigate => self
                .active_tab_index()
                .and_then(|index| self.tabs.get(index))
                .map(|tab| {
                    if tab.unloaded {
                        ""
                    } else {
                        tab.url.as_str()
                    }
                })
                .unwrap_or(""),
            CommandMode::NewTab => "",
            CommandMode::NewWorkspace => "New Space",
            CommandMode::RenameWorkspace(id) => self
                .workspaces
                .iter()
                .find(|workspace| workspace.id == id)
                .map(|workspace| workspace.name.as_str())
                .unwrap_or("Space"),
        };
        set_window_text(self.address_hwnd, initial_text);
        let cue = match mode {
            CommandMode::Navigate | CommandMode::NewTab => "Search or Enter URL...",
            CommandMode::NewWorkspace | CommandMode::RenameWorkspace(_) => "Workspace name...",
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
        let mut raw = get_window_text(self.address_hwnd);
        if let Some(i) = self.command_selected_index {
            if let Some(sugg) = self.command_suggestions().get(i) {
                if let Some(tab_index) = sugg.0 {
                    self.close_command();
                    self.switch_to(tab_index, true);
                    return;
                }
                raw = sugg.2.clone();
            }
        }
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

        }
    }

    fn create_folder_inline(&mut self) {
        let id = self.next_folder_id;
        self.next_folder_id += 1;
        self.folders.push(Folder {
            id,
            workspace_id: self.active_workspace,
            parent_id: None,
            name: "New Folder".to_string(),
            collapsed: false,
            pinned: false,
        });
        self.renaming_folder_id = Some(id);
        self.rename_buffer = "New Folder".to_string();
        self.rename_selected = true;
        self.save_state();
        self.layout();
        self.refresh();
        self.create_rename_edit(id);
    }

    fn rename_folder_inline(&mut self, folder_id: usize) {
        if let Some(folder) = self.folders.iter().find(|f| f.id == folder_id) {
            self.renaming_folder_id = Some(folder_id);
            self.rename_buffer = folder.name.clone();
            self.rename_selected = true;
            self.layout();
            self.refresh();
            self.create_rename_edit(folder_id);
        }
    }

    fn confirm_rename(&mut self) {
        if self.renaming_edit.is_some() {
            self.confirm_rename_from_edit();
            return;
        }
        if let Some(id) = self.renaming_folder_id.take() {
            let name = self.rename_buffer.clone();
            if !name.trim().is_empty() {
                if let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) {
                    folder.name = name.trim().to_string();
                }
            }
            self.rename_buffer.clear();
            self.rename_selected = false;
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
            if let Some(edit_hwnd) = self.renaming_edit.take() {
                unsafe {
                    let _ = WindowsAndMessaging::DestroyWindow(edit_hwnd);
                    let _ = SetFocus(Some(self.hwnd));
                }
            }
            self.rename_buffer.clear();
            self.rename_selected = false;
            self.save_state();
            self.refresh();
        }
    }

    fn confirm_rename_from_edit(&mut self) {
        if let Some(edit_hwnd) = self.renaming_edit.take() {
            let text = get_window_text(edit_hwnd);
            if let Some(id) = self.renaming_folder_id.take() {
                if !text.trim().is_empty() {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) {
                        folder.name = text.trim().to_string();
                    }
                }
            }
            unsafe {
                let _ = WindowsAndMessaging::DestroyWindow(edit_hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
            self.rename_buffer.clear();
            self.rename_selected = false;
            self.save_state();
            self.refresh();
        }
    }

    fn cancel_rename_from_edit(&mut self) {
        if let Some(edit_hwnd) = self.renaming_edit.take() {
            if let Some(id) = self.renaming_folder_id.take() {
                if let Some(folder) = self.folders.iter().find(|f| f.id == id) {
                    if folder.name == "New Folder" {
                        self.folders.retain(|f| f.id != id);
                    }
                }
            }
            unsafe {
                let _ = WindowsAndMessaging::DestroyWindow(edit_hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
            self.rename_buffer.clear();
            self.rename_selected = false;
            self.save_state();
            self.refresh();
        }
    }

    fn create_rename_edit(&mut self, folder_id: usize) {
        if let Some(existing) = self.renaming_edit.take() {
            unsafe {
                let _ = WindowsAndMessaging::DestroyWindow(existing);
            }
        }

        let row_rect = self.sidebar_row_rects()
            .into_iter()
            .find_map(|(row, rect)| match row {
                SidebarRow::Folder(id) if id == folder_id => Some(rect),
                _ => None,
            });

        if let Some(rect) = row_rect {
            unsafe {
                let hinstance = HINSTANCE(LibraryLoader::GetModuleHandleW(None).unwrap_or_default().0);
                let edit_hwnd = WindowsAndMessaging::CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("EDIT"),
                    w!(""),
                    WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | 0x0080 /* ES_AUTOHSCROLL */),
                    rect.left + 54,
                    rect.top + 5,
                    (rect.right - rect.left - 62).max(50),
                    22,
                    Some(self.hwnd),
                    None,
                    Some(hinstance),
                    None,
                );

                if let Ok(edit) = edit_hwnd {
                    let _ = WindowsAndMessaging::SendMessageW(
                        edit,
                        WM_SETFONT,
                        Some(WPARAM(self.fonts.body.0 as usize)),
                        Some(LPARAM(1)),
                    );

                    if let Some(folder) = self.folders.iter().find(|f| f.id == folder_id) {
                        set_window_text(edit, &folder.name);
                    }

                    let _ = WindowsAndMessaging::SendMessageW(
                        edit,
                        EM_SETSEL,
                        Some(WPARAM(0)),
                        Some(LPARAM(-1)),
                    );

                    OLD_RENAME_EDIT_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
                        edit,
                        GWLP_WNDPROC,
                        rename_edit_proc as *const () as isize,
                    ));

                    let _ = SetFocus(Some(edit));
                    self.renaming_edit = Some(edit);
                }
            }
        }
    }

    fn position_rename_edit(&self) {
        if let Some(edit_hwnd) = self.renaming_edit {
            if let Some(folder_id) = self.renaming_folder_id {
                let row_rect = self.sidebar_row_rects()
                    .into_iter()
                    .find_map(|(row, rect)| match row {
                        SidebarRow::Folder(id) if id == folder_id => Some(rect),
                        _ => None,
                    });
                if let Some(rect) = row_rect {
                    unsafe {
                        let _ = WindowsAndMessaging::SetWindowPos(
                            edit_hwnd,
                            None,
                            rect.left + 54,
                            rect.top + 5,
                            (rect.right - rect.left - 62).max(50),
                            22,
                            WindowsAndMessaging::SWP_NOZORDER | WindowsAndMessaging::SWP_NOACTIVATE,
                        );
                    }
                }
            }
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

    fn window_button_rects(&self) -> (RECT, RECT, RECT) {
        let rect = client_rect(self.hwnd);
        (
            RECT {
                left: rect.right - 138,
                top: 0,
                right: rect.right - 92,
                bottom: TOPBAR_HEIGHT,
            },
            RECT {
                left: rect.right - 92,
                top: 0,
                right: rect.right - 46,
                bottom: TOPBAR_HEIGHT,
            },
            RECT {
                left: rect.right - 46,
                top: 0,
                right: rect.right,
                bottom: TOPBAR_HEIGHT,
            },
        )
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
        let mut rows: Vec<(Option<usize>, String, String)> = Vec::new();
        let now = current_timestamp();

        // 1. Get open tabs matching query
        for tab_index in self.active_workspace_tabs() {
            if let Some(tab) = self.tabs.get(tab_index) {
                if query.is_empty()
                    || tab.url.to_ascii_lowercase().contains(&query)
                    || tab.title.to_ascii_lowercase().contains(&query)
                {
                    let norm_tab = normalize_url_for_dedup(&tab.url);
                    if rows.iter().any(|row| normalize_url_for_dedup(&row.2) == norm_tab) {
                        continue;
                    }
                    rows.push((Some(tab_index), tab.title.clone(), tab.url.clone()));
                }
            }
        }

        // 2. Get visited history sites matching query, sorted by frecency score descending
        let mut matched_history: Vec<&VisitedSite> = self.visited_sites.iter()
            .filter(|site| {
                if query.is_empty() {
                    true
                } else {
                    site.url.to_ascii_lowercase().contains(&query) ||
                    extract_search_query(&site.url)
                        .map(|q| q.to_ascii_lowercase().contains(&query))
                        .unwrap_or(false)
                }
            })
            .collect();
        
        matched_history.sort_by_cached_key(|site| {
            std::cmp::Reverse(calculate_frecency(site.visit_count, site.last_visit_time, now))
        });

        // Add history suggestions to rows
        for site in &matched_history {
            let norm_site = normalize_url_for_dedup(&site.url);
            if rows.iter().any(|row| normalize_url_for_dedup(&row.2) == norm_site) {
                continue;
            }
            rows.push((None, label_for_url(&site.url), site.url.clone()));
        }

        // 3. If there is a query, sort the results to prioritize prefix matches
        if !query.is_empty() {
            rows.sort_by_key(|row| {
                let url = &row.2;
                let title = &row.1;
                // Group 0: Direct prefix match on URL or match on search query
                if url.to_ascii_lowercase().starts_with(&query) {
                    return 0;
                }
                if let Some(q) = extract_search_query(url) {
                    if q.to_ascii_lowercase().starts_with(&query) {
                        return 0;
                    }
                }
                // Group 1: Cleaned prefix match on URL
                if clean_all_prefixes(url).to_ascii_lowercase().starts_with(&query) {
                    return 1;
                }
                // Group 2: Prefix match on title
                if title.to_ascii_lowercase().starts_with(&query) {
                    return 2;
                }
                // Group 3: Contains match
                3
            });
        }

        rows.truncate(50);
        rows
    }

    fn try_autofill(&mut self, current_text: &str) {
        if current_text.is_empty() {
            return;
        }
        let suggestions = self.command_suggestions();
        if let Some(top) = suggestions.first() {
            let url = &top.2;
            let clean = clean_all_prefixes(url);
            let display_url = if clean.to_ascii_lowercase().starts_with(&current_text.to_ascii_lowercase()) {
                clean.to_string()
            } else if url.to_ascii_lowercase().starts_with(&current_text.to_ascii_lowercase()) {
                url.to_string()
            } else {
                return;
            };

            let remaining = &display_url[current_text.len()..];
            if !remaining.is_empty() {
                let start_sel = current_text.len() as u32;
                let full = format!("{}{}", current_text, remaining);
                set_window_text(self.address_hwnd, &full);
                unsafe {
                    let _ = WindowsAndMessaging::SendMessageW(
                        self.address_hwnd,
                        EM_SETSEL,
                        Some(WPARAM(start_sel as usize)),
                        Some(LPARAM(-1)),
                    );
                }
                self.command_selected_index = Some(0);
                self.last_address_text = full;
            }
        }
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
        let last = self.last_bounds_rect.get();
        let size_changed = bounds.left != last.left
            || bounds.right != last.right
            || bounds.top != last.top
            || bounds.bottom != last.bottom;

        let needs_clipping = self.sidebar_mode == SidebarMode::Overlay
            || (self.sidebar_mode == SidebarMode::Hidden
                && self.sidebar_expand_mode == SidebarMode::Overlay
                && self.sidebar_target >= SIDEBAR_EXPANDED);
        let clip_changed = needs_clipping
            && sidebar_width > 0
            && ((sidebar_width as f32 - self.last_clip_width.get()).abs() > 1.0 || size_changed);
        let was_clipped = self.last_clip_width.get() != 0.0;
        let should_clear = (!needs_clipping || sidebar_width <= 0) && was_clipped;
        for (i, tab) in self.tabs.iter().enumerate() {
            unsafe {
                let is_active = Some(i) == self.active_tab_index();
                if is_active {
                    let _ = tab.controller.SetBounds(bounds);
                    let _ = WindowsAndMessaging::SetWindowPos(
                        tab.child_hwnd,
                        None,
                        bounds.left,
                        bounds.top,
                        bounds.right - bounds.left,
                        bounds.bottom - bounds.top,
                        WindowsAndMessaging::SWP_NOZORDER | WindowsAndMessaging::SWP_NOACTIVATE,
                    );
                }
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
                    .SetIsVisible(is_active && !tab.unloaded);
            }
        }
        if clip_changed {
            self.last_clip_width.set(sidebar_width as f32);
        } else if should_clear {
            self.last_clip_width.set(0.0);
        }
        if size_changed {
            self.last_bounds_rect.set(bounds);
        }
        self.position_rename_edit();
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

            let is_unloaded = self.tabs.get(self.active).map(|t| t.unloaded).unwrap_or(false);
            if self.active_tab_index().is_none() || is_unloaded {
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
                    if tab.unloaded || tab.url.is_empty() {
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

            let (min_btn, max_btn, close_btn) = self.window_button_rects();

            // Draw Minimize Button
            let min_hover = self.hover_target == Some(HoverTarget::MinButton);
            if min_hover {
                let _ = FillRect(hdc, &min_btn, self.brushes.hover);
            }
            {
                let cx = (min_btn.left + min_btn.right) / 2;
                let cy = (min_btn.top + min_btn.bottom) / 2;
                fill_rect(hdc, RECT { left: cx - 6, top: cy, right: cx + 6, bottom: cy + 1 }, COLOR_TEXT);
            }

            // Draw Maximize Button
            let max_hover = self.hover_target == Some(HoverTarget::MaxButton);
            if max_hover {
                let _ = FillRect(hdc, &max_btn, self.brushes.hover);
            }
            {
                let cx = (max_btn.left + max_btn.right) / 2;
                let cy = (max_btn.top + max_btn.bottom) / 2;
                fill_rect(hdc, RECT { left: cx - 5, top: cy - 5, right: cx + 5, bottom: cy - 4 }, COLOR_TEXT);
                fill_rect(hdc, RECT { left: cx - 5, top: cy + 4, right: cx + 5, bottom: cy + 5 }, COLOR_TEXT);
                fill_rect(hdc, RECT { left: cx - 5, top: cy - 4, right: cx - 4, bottom: cy + 4 }, COLOR_TEXT);
                fill_rect(hdc, RECT { left: cx + 4, top: cy - 4, right: cx + 5, bottom: cy + 4 }, COLOR_TEXT);
            }

            // Draw Close Button
            let close_hover = self.hover_target == Some(HoverTarget::CloseButton);
            if close_hover {
                fill_rect(hdc, close_btn, 0xE81123);
            }
            {
                let cx = (close_btn.left + close_btn.right) / 2;
                let cy = (close_btn.top + close_btn.bottom) / 2;
                let color = if close_hover { 0xffffff } else { COLOR_TEXT };
                for i in -4..=4 {
                    fill_rect(hdc, RECT { left: cx + i, top: cy + i, right: cx + i + 1, bottom: cy + i + 1 }, color);
                    fill_rect(hdc, RECT { left: cx + i, top: cy - i, right: cx + i + 1, bottom: cy - i + 1 }, color);
                }
            }

            if sidebar_width > 92 {
                self.paint_workspace_header(hdc);
                let has_pinned = self.folders.iter().any(|f| f.workspace_id == self.active_workspace && f.pinned)
                    || self.tabs.iter().any(|t| t.workspace_id == self.active_workspace && t.pinned);
                if !has_pinned {
                    if let Some(rect) = self.pinned_section_rect() {
                        if let Ok(large_pin_font) = create_font_with_face(58, 400, w!("Segoe Fluent Icons")) {
                            draw_icon_glyph(
                                hdc,
                                &large_pin_font,
                                glyph(0xE718).as_str(),
                                RECT {
                                    left: rect.left,
                                    top: rect.top,
                                    right: rect.right,
                                    bottom: rect.bottom,
                                },
                                COLOR_ACCENT,
                            );
                            let _ = DeleteObject(HGDIOBJ(large_pin_font.0));
                        }
                    }
                }
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
            }

            if self.settings_open {
                self.paint_settings_menu(hdc);
            }
        }
    }

    fn paint_cached_background(&self, hdc: HDC, rect: RECT) {
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        
        let has_cache = self.background_cache.borrow().is_some();
        if !has_cache {
            *self.background_cache.borrow_mut() = render_aster_background_bitmap(width, height);
        }
        
        if let Some(bitmap) = self.background_cache.borrow().as_ref() {
            unsafe {
                let mem_dc = CreateCompatibleDC(Some(hdc));
                if !mem_dc.is_invalid() {
                    let old = SelectObject(mem_dc, HGDIOBJ(bitmap.handle.0));
                    let _ = StretchBlt(
                        hdc,
                        rect.left,
                        rect.top,
                        width,
                        height,
                        Some(mem_dc),
                        0,
                        0,
                        bitmap.width,
                        bitmap.height,
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
            let width = menu.rect.right - menu.rect.left;
            let height = menu.rect.bottom - menu.rect.top;
            let local_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            fill_round_rect(hdc, local_rect, 0x111111, 10);
            draw_outline(hdc, local_rect, 0x343434, 10);
            for (index, item) in menu.items.iter().enumerate() {
                let row = RECT {
                    left: 6,
                    top: 6 + index as i32 * MENU_ROW_HEIGHT,
                    right: width - 6,
                    bottom: 6 + (index as i32 + 1) * MENU_ROW_HEIGHT,
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

            let suggestions = self.command_suggestions();
            let total_rows = suggestions.len();
            for (i, (tab_index, title, url)) in
                suggestions.into_iter().skip(self.command_scroll_offset).take(5).enumerate()
            {
                let row_index = i;
                let mut row = self.command_tab_row_rect(row_index);
                row.left -= self.command_popup_rect().left;
                row.right -= self.command_popup_rect().left;
                row.top -= self.command_popup_rect().top;
                row.bottom -= self.command_popup_rect().top;

                let global_index = i + self.command_scroll_offset;
                if Some(global_index) == self.command_selected_index {
                    fill_round_rect(hdc, row, COLOR_SURFACE_HOVER, 8);
                }

                if tab_index == self.active_tab_index() {
                    let indicator = RECT {
                        left: row.left,
                        top: row.top + 8,
                        right: row.left + 4,
                        bottom: row.bottom - 8,
                    };
                    fill_round_rect(hdc, indicator, COLOR_ACCENT, 2);
                }
                let favicon = RECT {
                    left: row.left + 14,
                    top: row.top + 8,
                    right: row.left + 30,
                    bottom: row.top + 24,
                };
                let mut favicon_drawn = false;
                if let Some(index) = tab_index.and_then(|index| self.tabs.get(index)) {
                    draw_tab_favicon(hdc, &self.fonts.small, favicon, index);
                    favicon_drawn = true;
                } else {
                    let host = display_host(&url);
                    if !host.is_empty() {
                        if let Some(matching_tab) = self.tabs.iter().find(|t| {
                            t.favicon_bitmap.is_some() && display_host(&t.url) == host
                        }) {
                            draw_tab_favicon(hdc, &self.fonts.small, favicon, matching_tab);
                            favicon_drawn = true;
                        }
                    }
                }

                if !favicon_drawn {
                    let is_search = extract_search_query(&url).is_some();
                    let icon_glyph = if is_search {
                        glyph(0xE721)
                    } else {
                        glyph(0xE774)
                    };
                    draw_icon_glyph(
                        hdc,
                        &self.fonts.toolbar_icon,
                        icon_glyph.as_str(),
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
            }

            if total_rows > 5 {
                let visible_ratio = 5.0 / total_rows as f32;
                let scroll_ratio = self.command_scroll_offset as f32 / total_rows as f32;
                let max_rows = 5;
                let track_height = (max_rows * 40) as f32;
                let track_top = 64.0;
                
                let thumb_height = (track_height * visible_ratio).max(20.0);
                let thumb_top = track_top + (track_height * scroll_ratio);
                
                let scrollbar_rect = RECT {
                    left: panel.right - 8,
                    top: thumb_top as i32,
                    right: panel.right - 4,
                    bottom: (thumb_top + thumb_height) as i32,
                };
                fill_round_rect(hdc, scrollbar_rect, 0x333333, 2);
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

    fn paint_sidebar_label(&self, hdc: HDC, _label: SidebarLabel, rect: RECT) {
        unsafe {
            fill_rect(
                hdc,
                RECT {
                    left: rect.left + 12,
                    top: rect.top + 12,
                    right: rect.right - 8,
                    bottom: rect.top + 13,
                },
                0x2a2a2a,
            );
        }
    }

    fn paint_folder_row(&self, hdc: HDC, folder_id: usize, rect: RECT) {
        let Some(folder) = self.folders.iter().find(|folder| folder.id == folder_id) else {
            return;
        };
        let is_renaming = self.renaming_folder_id == Some(folder_id);
        let display_name = if is_renaming {
            if self.rename_selected {
                self.rename_buffer.clone()
            } else {
                format!("{}|", self.rename_buffer)
            }
        } else {
            folder.name.clone()
        };
        unsafe {
            let depth = self.folder_depth(folder_id);
            let shift = (depth * 16) as i32;
            let item = RECT {
                left: rect.left + 2 + shift,
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
                if !is_renaming {
                    draw_text(
                        hdc,
                        &self.fonts.body,
                        &display_name,
                        RECT {
                            left: item.left + 56,
                            top: item.top,
                            right: item.right - 8,
                            bottom: item.bottom,
                        },
                        COLOR_MUTED,
                    );
                }
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
                if !is_renaming {
                    draw_text(
                        hdc,
                        &self.fonts.body,
                        &display_name,
                        RECT {
                            left: item.left + 56,
                            top: item.top,
                            right: item.right - 8,
                            bottom: item.bottom,
                        },
                        COLOR_MUTED,
                    );
                }
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


    fn paint_tab(&self, hdc: HDC, index: usize, tab: &Tab, item: RECT) {
        unsafe {
            let mut item = item;
            let depth = self.tab_depth(index);
            if depth > 0 {
                item.left += (depth * 16) as i32;
            }
            if self.hover_tab == Some(index) || Some(index) == self.active_tab_index() {
                fill_round_rect(hdc, item, 0x151515, 10);
            }
            let mut text_left = item.left + 40;
            let show_pin = tab.pinned && tab.folder_id.is_none();
            if show_pin {
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
            let favicon_left = if show_pin {
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
                let close_glyph = if tab.pinned {
                    if tab.unloaded {
                        glyph(0xE711)
                    } else {
                        glyph(0xE108)
                    }
                } else {
                    glyph(0xE711)
                };
                draw_icon_glyph(
                    hdc,
                    &self.fonts.icon,
                    close_glyph.as_str(),
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
        let clicked_renaming_folder = if let Some(folder_id) = self.renaming_folder_id {
            matches!(self.hit_sidebar(x, y), Some(SidebarHit::Folder(hit_id)) if hit_id == folder_id)
        } else {
            false
        };

        if self.renaming_folder_id.is_some() {
            if clicked_renaming_folder {
                return;
            } else {
                self.confirm_rename();
            }
        }

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

        let (min_btn, max_btn, close_btn) = self.window_button_rects();
        if point_in_rect(x, y, min_btn) {
            unsafe {
                let _ = WindowsAndMessaging::ShowWindow(self.hwnd, WindowsAndMessaging::SW_MINIMIZE);
            }
            return;
        }
        if point_in_rect(x, y, max_btn) {
            unsafe {
                if WindowsAndMessaging::IsZoomed(self.hwnd).as_bool() {
                    let _ = WindowsAndMessaging::ShowWindow(self.hwnd, WindowsAndMessaging::SW_RESTORE);
                } else {
                    let _ = WindowsAndMessaging::ShowWindow(self.hwnd, WindowsAndMessaging::SW_MAXIMIZE);
                }
            }
            return;
        }
        if point_in_rect(x, y, close_btn) {
            unsafe {
                let _ = WindowsAndMessaging::PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return;
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
                        if self.rename_selected {
                            self.rename_selected = false;
                            self.refresh();
                        }
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
                            if let Some(tab) = self.tabs.get(index) {
                                if tab.pinned && tab.unloaded {
                                    self.delete_pin(index);
                                } else {
                                    self.close_tab(index);
                                }
                            }
                        } else if index != self.active || self.tabs[index].unloaded {
                            self.switch_to(index, true);
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
                if tab.pinned {
                    labels.push((MENU_TAB_DELETE_PIN, "Delete Pin".to_string()));
                }
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
            unsafe {
                let _ = WindowsAndMessaging::ShowWindow(self.overlay_menu_hwnd, WindowsAndMessaging::SW_HIDE);
            }
            return;
        }
        let rect = client_rect(self.hwnd);
        let height = 12 + items.len() as i32 * MENU_ROW_HEIGHT;
        let left = x.min(rect.right - MENU_WIDTH - 8).max(8);
        let top = y.min(rect.bottom - height - 8).max(TOPBAR_HEIGHT + 8);
        let menu_rect = RECT {
            left,
            top,
            right: left + MENU_WIDTH,
            bottom: top + height,
        };
        self.overlay_menu = Some(OverlayMenu {
            rect: menu_rect,
            target,
            items,
        });

        unsafe {
            let flags = WindowsAndMessaging::SWP_NOZORDER;
            let _ = WindowsAndMessaging::SetWindowPos(
                self.overlay_menu_hwnd,
                Some(HWND_TOP),
                left,
                top,
                MENU_WIDTH,
                height,
                flags,
            );
            let _ = WindowsAndMessaging::ShowWindow(self.overlay_menu_hwnd, WindowsAndMessaging::SW_SHOW);
            let _ = InvalidateRect(Some(self.overlay_menu_hwnd), None, false);
            let _ = SetFocus(Some(self.overlay_menu_hwnd));
        }
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
        unsafe {
            let _ = WindowsAndMessaging::ShowWindow(self.overlay_menu_hwnd, WindowsAndMessaging::SW_HIDE);
        }
        if !point_in_rect(x, y, menu.rect) {
            self.overlay_menu = None;
            self.refresh();
            return true;
        }
        let row_index = (y - menu.rect.top - 6) / MENU_ROW_HEIGHT;
        if row_index < 0 || row_index as usize >= menu.items.len() {
            self.overlay_menu = None;
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
            MENU_WORKSPACE_NEW_FOLDER => self.create_folder_inline(),
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
                    self.rename_folder_inline(folder_id);
                }
            }
            MENU_FOLDER_DELETE => {
                if let SidebarHit::Folder(folder_id) = hit {
                    self.folders.retain(|folder| folder.id != folder_id);
                    for f in &mut self.folders {
                        if f.parent_id == Some(folder_id) {
                            f.parent_id = None;
                        }
                    }
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
                    self.propagate_folder_pinning(folder_id, true);
                    self.save_state();
                }
            }
            MENU_FOLDER_UNPIN => {
                if let SidebarHit::Folder(folder_id) = hit {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.id == folder_id) {
                        folder.pinned = false;
                    }
                    self.propagate_folder_pinning(folder_id, false);
                    self.save_state();
                }
            }
            MENU_TAB_PIN | MENU_TAB_UNPIN | MENU_TAB_REMOVE_FOLDER | MENU_TAB_CLOSE | MENU_TAB_DELETE_PIN
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
                        MENU_TAB_DELETE_PIN => self.delete_pin(index),
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
        let mut should_create_ghost = false;
        let mut drag_coords = None;
        if let Some(drag) = self.drag_state.as_mut() {
            drag.current_x = x;
            drag.current_y = y;
            if !drag.active && (x - drag.start_x).abs() + (y - drag.start_y).abs() > 6 {
                drag.active = true;
                should_create_ghost = true;
            }
            drag_coords = Some((drag.current_x, drag.current_y));
        }

        if should_create_ghost {
            unsafe {
                let _ = SetCapture(self.hwnd);
            }
            self.create_drag_ghost();
        }

        if let Some((cx, cy)) = drag_coords {
            if let Some(hwnd) = self.drag_ghost_hwnd.get() {
                let mut screen_pt = POINT { x: cx + 10, y: cy + 10 };
                unsafe {
                    let _ = Gdi::ClientToScreen(self.hwnd, &mut screen_pt);
                    let _ = WindowsAndMessaging::SetWindowPos(
                        hwnd,
                        None,
                        screen_pt.x,
                        screen_pt.y,
                        0,
                        0,
                        WindowsAndMessaging::SWP_NOSIZE
                            | WindowsAndMessaging::SWP_NOZORDER
                            | WindowsAndMessaging::SWP_NOACTIVATE,
                    );
                }
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

        let (min_btn, max_btn, close_btn) = self.window_button_rects();
        if point_in_rect(x, y, self.logo_rect()) {
            self.hover_target = Some(HoverTarget::Logo);
        } else if self.new_tab_opacity() > 0.6 && point_in_rect(x, y, self.new_tab_rect()) {
            self.hover_target = Some(HoverTarget::NewTab);
        } else if point_in_rect(x, y, min_btn) {
            self.hover_target = Some(HoverTarget::MinButton);
        } else if point_in_rect(x, y, max_btn) {
            self.hover_target = Some(HoverTarget::MaxButton);
        } else if point_in_rect(x, y, close_btn) {
            self.hover_target = Some(HoverTarget::CloseButton);
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
        if let Some(hwnd) = self.drag_ghost_hwnd.take() {
            unsafe {
                let _ = WindowsAndMessaging::DestroyWindow(hwnd);
                CURRENT_DRAG_GHOST_BITMAP = None;
            }
        }
        self.drop_target = Some(DropTarget::None);
        if !drag.active {
            return false;
        }
        self.handle_drop(drag.source, x, y);
        true
    }
    fn handle_drop(&mut self, source: DragSource, x: i32, y: i32) {
        let hit = self.hit_sidebar(x, y);
        let divider_y = self.sidebar_row_rects().iter()
            .find(|(row, _)| matches!(row, SidebarRow::Label(SidebarLabel::Tabs)))
            .map(|(_, rect)| rect.top)
            .unwrap_or(SIDEBAR_ROWS_TOP + 72);

        let hit = if hit.is_none() && x >= 0 && (x as f32) < self.sidebar_width {
            if y <= divider_y {
                Some(SidebarHit::PinnedSection)
            } else {
                None
            }
        } else {
            hit
        };

        let is_normal_fallback = hit.is_none() && x >= 0 && (x as f32) < self.sidebar_width && y > divider_y;

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
                            folder.id == folder_id && folder.workspace_id == dragged_workspace
                        }) {
                            if let Some(tab) = self.tabs.get_mut(from_index) {
                                tab.folder_id = Some(folder_id);
                                tab.pinned = folder.pinned;
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
                        tab.pinned = target_pinned;
                        tab.folder_id = target_folder;
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
                    _ => {
                        if is_normal_fallback {
                            let mut tab = self.tabs.remove(from_index);
                            tab.pinned = false;
                            tab.folder_id = None;
                            self.tabs.push(tab);
                        }
                    }
                }
            }
            DragSource::Folder(from_folder_id) => {
                match hit {
                    Some(SidebarHit::PinnedSection) | Some(SidebarHit::WorkspaceHeader) => {
                        if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                            folder.pinned = true;
                            folder.parent_id = None;
                        }
                        self.propagate_folder_pinning(from_folder_id, true);
                    }
                    Some(SidebarHit::Folder(target_folder_id)) => {
                        if target_folder_id == from_folder_id || self.is_descendant_of(target_folder_id, from_folder_id) {
                            return;
                        }
                        let target_pinned = self
                            .folders
                            .iter()
                            .find(|f| f.id == target_folder_id)
                            .map(|f| f.pinned);
                        if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                            folder.parent_id = Some(target_folder_id);
                            if let Some(target_pinned) = target_pinned {
                                folder.pinned = target_pinned;
                            }
                        }
                        if let Some(target_pinned) = target_pinned {
                            self.propagate_folder_pinning(from_folder_id, target_pinned);
                        }
                    }
                    _ => {
                        if is_normal_fallback {
                            if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                                folder.pinned = false;
                                folder.parent_id = None;
                            }
                            self.propagate_folder_pinning(from_folder_id, false);
                        }
                    }
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
            });

            CURRENT_DRAG_GHOST_BITMAP = Some(bitmap);

            let mut screen_pt = POINT { x: drag.current_x + 10, y: drag.current_y + 10 };
            let _ = Gdi::ClientToScreen(self.hwnd, &mut screen_pt);

            let ghost_hwnd = WindowsAndMessaging::CreateWindowExW(
                WINDOW_EX_STYLE(0x00080000 | 0x00000020 | 0x00000080 | 0x00000008), // WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST
                w!("STATIC"),
                w!(""),
                WS_POPUP | WS_VISIBLE,
                screen_pt.x,
                screen_pt.y,
                ghost_width,
                ghost_height,
                Some(self.hwnd),
                None,
                Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None).unwrap().0)),
                None,
            ).ok();

            if let Some(hwnd) = ghost_hwnd {
                let _ = WindowsAndMessaging::SetLayeredWindowAttributes(
                    hwnd,
                    COLORREF(0),
                    180,
                    WindowsAndMessaging::LWA_ALPHA,
                );
                OLD_DRAG_GHOST_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    GWLP_WNDPROC,
                    drag_ghost_proc as *const () as isize,
                ));
                self.drag_ghost_hwnd.set(Some(hwnd));
            }
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
        if self.drag_state.is_some() {
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

    fn regenerate_background(&self) {
        let rect = client_rect(self.hwnd);
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
            self.refresh();
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

fn profile_path() -> String {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let mut path = PathBuf::from(appdata);
        path.push("Aster");
        let _ = std::fs::create_dir_all(&path);
        path.push(".aster-profile");
        path.to_string_lossy().into_owned()
    } else {
        ".aster-profile".to_string()
    }
}

fn create_environment() -> AppResult<ICoreWebView2Environment> {
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|handler| unsafe {
            let path = profile_path();
            let user_data = CoTaskMemPWSTR::from(path.as_str());
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
            style: WindowsAndMessaging::CS_DBLCLKS,
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
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            APP_NAME,
            WS_OVERLAPPEDWINDOW | WS_CLIPSIBLINGS | WindowsAndMessaging::WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            820,
            None,
            None,
            Some(hinstance),
            None,
        )?;

        // Extend frame into client area to keep native shadows
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 1,
            cyBottomHeight: 0,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // Force OS to update the non-client area frame changes
        let _ = WindowsAndMessaging::SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            WindowsAndMessaging::SWP_FRAMECHANGED
                | WindowsAndMessaging::SWP_NOMOVE
                | WindowsAndMessaging::SWP_NOSIZE
                | WindowsAndMessaging::SWP_NOZORDER
                | WindowsAndMessaging::SWP_NOACTIVATE,
        );
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
            WINDOW_STYLE(WS_CHILD.0 | WS_CLIPSIBLINGS.0 | 0x00000100 /* SS_NOTIFY */),
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

fn create_overlay_menu(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_CLIPSIBLINGS.0 | 0x00000100 /* SS_NOTIFY */),
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        OLD_OVERLAY_MENU_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            overlay_menu_proc as *const () as isize,
        ));
        Ok(hwnd)
    }
}

unsafe extern "system" fn overlay_menu_proc(
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
                with_app(parent, |app| {
                    if let Some(menu) = &app.overlay_menu {
                        app.paint_overlay_menu(hdc, menu);
                    }
                });
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
                    if let Some(menu) = &app.overlay_menu {
                        let parent_x = x + menu.rect.left;
                        let parent_y = y + menu.rect.top;
                        let _ = app.handle_overlay_click(parent_x, parent_y);
                    }
                });
            }
            LRESULT(0)
        }
        WindowsAndMessaging::WM_KILLFOCUS => {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| {
                    app.overlay_menu = None;
                    let _ = WindowsAndMessaging::ShowWindow(app.overlay_menu_hwnd, WindowsAndMessaging::SW_HIDE);
                    app.refresh();
                });
            }
            LRESULT(0)
        }
        _ => WindowsAndMessaging::CallWindowProcW(
            OLD_OVERLAY_MENU_PROC,
            hwnd,
            msg,
            w_param,
            l_param,
        ),
    }
}

unsafe extern "system" fn drag_ghost_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(bitmap) = CURRENT_DRAG_GHOST_BITMAP {
                let mem_dc = CreateCompatibleDC(Some(hdc));
                let old = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
                let mut rect = RECT::default();
                let _ = WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
                let _ = BitBlt(
                    hdc,
                    0,
                    0,
                    rect.right,
                    rect.bottom,
                    Some(mem_dc),
                    0,
                    0,
                    SRCCOPY,
                );
                let _ = SelectObject(mem_dc, old);
                let _ = DeleteDC(mem_dc);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => WindowsAndMessaging::CallWindowProcW(
            OLD_DRAG_GHOST_PROC,
            hwnd,
            msg,
            w_param,
            l_param,
        ),
    }
}

unsafe extern "system" fn rename_edit_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN && w_param.0 as u32 == VK_RETURN.0 as u32 {
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| {
                app.confirm_rename_from_edit();
            });
        }
        return LRESULT(0);
    }
    if msg == WM_KEYDOWN && w_param.0 as u32 == VK_ESCAPE.0 as u32 {
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| {
                app.cancel_rename_from_edit();
            });
        }
        return LRESULT(0);
    }
    if msg == WM_CHAR && w_param.0 as u32 == VK_RETURN.0 as u32 {
        return LRESULT(0);
    }
    if msg == WindowsAndMessaging::WM_KILLFOCUS {
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| {
                app.confirm_rename_from_edit();
            });
        }
    }
    WindowsAndMessaging::CallWindowProcW(OLD_RENAME_EDIT_PROC, hwnd, msg, w_param, l_param)
}

fn is_typing_key(key: u32) -> bool {
    if (0x70..=0x87).contains(&key) {
        return false;
    }
    let excluded = [
        0x08, // VK_BACK
        0x09, // VK_TAB
        0x0D, // VK_RETURN
        0x10, // VK_SHIFT
        0x11, // VK_CONTROL
        0x12, // VK_MENU (Alt)
        0x14, // VK_CAPITAL (Caps Lock)
        0x1B, // VK_ESCAPE
        0x21, // VK_PRIOR (Page Up)
        0x22, // VK_NEXT (Page Down)
        0x23, // VK_END
        0x24, // VK_HOME
        0x25, // VK_LEFT
        0x26, // VK_UP
        0x27, // VK_RIGHT
        0x28, // VK_DOWN
        0x2D, // VK_INSERT
        0x2E, // VK_DELETE
        0x5B, // VK_LWIN
        0x5C, // VK_RWIN
        0x90, // VK_NUMLOCK
        0x91, // VK_SCROLL
    ];
    !excluded.contains(&key)
}

unsafe extern "system" fn address_bar_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        let key = w_param.0 as u32;
        if key == VK_RETURN.0 as u32 {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| app.navigate_active_from_address());
            }
            return LRESULT(0);
        }
        if key == VK_ESCAPE.0 as u32 {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| app.close_command());
            }
            return LRESULT(0);
        }
        if key == 0x09 { // VK_TAB
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| {
                    if let Some(i) = app.command_selected_index {
                        if let Some(sugg) = app.command_suggestions().get(i) {
                            set_window_text(app.address_hwnd, &sugg.2);
                            app.last_address_text = sugg.2.clone();
                            unsafe {
                                let _ = WindowsAndMessaging::SendMessageW(
                                    app.address_hwnd,
                                    EM_SETSEL,
                                    Some(WPARAM(sugg.2.len())),
                                    Some(LPARAM(-1)),
                                );
                            }
                        }
                    }
                });
            }
            return LRESULT(0);
        }
        if key == 0x26 || key == 0x28 { // VK_UP or VK_DOWN
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| {
                    let total = app.command_suggestions().len();
                    if total > 0 {
                        let current = app.command_selected_index.unwrap_or(0);
                        let next = if key == 0x28 {
                            (current + 1).min(total - 1)
                        } else {
                            current.saturating_sub(1)
                        };
                        app.command_selected_index = Some(next);
                        if next < app.command_scroll_offset {
                            app.command_scroll_offset = next;
                        } else if next >= app.command_scroll_offset + 5 {
                            app.command_scroll_offset = next - 4;
                        }
                        unsafe {
                            let _ = InvalidateRect(Some(app.command_hwnd), None, false);
                        }
                    }
                });
            }
            return LRESULT(0);
        }
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| {
                if key == 8 || key == 46 { // VK_BACK or VK_DELETE
                    app.is_deleting = true;
                } else {
                    app.is_deleting = false;
                }
                if is_typing_key(key) {
                    app.has_typed = true;
                }
            });
        }
    }
    if msg == WM_MOUSEWHEEL {
        let delta = hiword(w_param.0 as u32) as i16 as i32;
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| {
                let total = app.command_suggestions().len();
                if total > 5 {
                    if delta < 0 {
                        app.command_scroll_offset = (app.command_scroll_offset + 1).min(total - 5);
                    } else {
                        app.command_scroll_offset = app.command_scroll_offset.saturating_sub(1);
                    }
                    unsafe {
                        let _ = InvalidateRect(Some(app.command_hwnd), None, false);
                    }
                }
            });
        }
        return LRESULT(0);
    }
    if msg == WM_CHAR && w_param.0 as u32 == VK_RETURN.0 as u32 {
        return LRESULT(0);
    }
    if msg == WindowsAndMessaging::WM_KILLFOCUS {
        if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
            with_app(parent, |app| {
                let next_focus = HWND(w_param.0 as _);
                if next_focus != app.command_hwnd {
                    app.close_command();
                }
            });
        }
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
                                app.switch_to(tab_index, true);
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
        WindowsAndMessaging::WM_KILLFOCUS => {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| {
                    let next_focus = HWND(w_param.0 as _);
                    if next_focus != app.address_hwnd {
                        app.close_command();
                    }
                });
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(w_param.0 as u32) as i16 as i32;
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| {
                    let total = app.command_suggestions().len();
                    if total > 5 {
                        if delta < 0 {
                            app.command_scroll_offset = (app.command_scroll_offset + 1).min(total - 5);
                        } else {
                            app.command_scroll_offset = app.command_scroll_offset.saturating_sub(1);
                        }
                        unsafe {
                            let _ = InvalidateRect(Some(app.command_hwnd), None, false);
                        }
                    }
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
        WindowsAndMessaging::WM_GETMINMAXINFO => {
            unsafe {
                let mmi = &mut *(l_param.0 as *mut WindowsAndMessaging::MINMAXINFO);
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut monitor_info = MONITORINFO {
                    cbSize: mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                    let work_area = monitor_info.rcWork;
                    let monitor_area = monitor_info.rcMonitor;
                    
                    mmi.ptMaxPosition.x = work_area.left - monitor_area.left;
                    mmi.ptMaxPosition.y = work_area.top - monitor_area.top;
                    mmi.ptMaxSize.x = work_area.right - work_area.left;
                    mmi.ptMaxSize.y = work_area.bottom - work_area.top;
                }
            }
            LRESULT(0)
        }
        WindowsAndMessaging::WM_NCCALCSIZE => {
            if w_param.0 != 0 {
                let is_maximized = unsafe { WindowsAndMessaging::IsZoomed(hwnd).as_bool() };
                if is_maximized {
                    unsafe {
                        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                        let mut monitor_info = MONITORINFO {
                            cbSize: mem::size_of::<MONITORINFO>() as u32,
                            ..Default::default()
                        };
                        if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                            let params = &mut *(l_param.0 as *mut WindowsAndMessaging::NCCALCSIZE_PARAMS);
                            params.rgrc[0] = monitor_info.rcWork;
                        }
                    }
                }
                LRESULT(0)
            } else {
                LRESULT(0)
            }
        }
        WindowsAndMessaging::WM_NCHITTEST => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            let mut pt = POINT { x, y };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            let rect = client_rect(hwnd);
            let is_maximized = unsafe { WindowsAndMessaging::IsZoomed(hwnd).as_bool() };
            let border_width = if is_maximized { 0 } else { 8 };

            if pt.y < border_width {
                if pt.x < border_width {
                    return LRESULT(WindowsAndMessaging::HTTOPLEFT as isize);
                }
                if pt.x > rect.right - border_width {
                    return LRESULT(WindowsAndMessaging::HTTOPRIGHT as isize);
                }
                return LRESULT(WindowsAndMessaging::HTTOP as isize);
            }
            if pt.y > rect.bottom - border_width {
                if pt.x < border_width {
                    return LRESULT(WindowsAndMessaging::HTBOTTOMLEFT as isize);
                }
                if pt.x > rect.right - border_width {
                    return LRESULT(WindowsAndMessaging::HTBOTTOMRIGHT as isize);
                }
                return LRESULT(WindowsAndMessaging::HTBOTTOM as isize);
            }
            if pt.x < border_width {
                return LRESULT(WindowsAndMessaging::HTLEFT as isize);
            }
            if pt.x > rect.right - border_width {
                return LRESULT(WindowsAndMessaging::HTRIGHT as isize);
            }

            if pt.y < TOPBAR_HEIGHT {
                let mut is_interactive = false;
                with_app(hwnd, |app| {
                    let (back, forward, reload) = app.top_button_rects();
                    let logo = app.logo_rect();
                    let new_tab = app.new_tab_rect();
                    let address = app.address_pill_rect();
                    let (min_btn, max_btn, close_btn) = app.window_button_rects();

                    if point_in_rect(pt.x, pt.y, logo)
                        || point_in_rect(pt.x, pt.y, new_tab)
                        || point_in_rect(pt.x, pt.y, back)
                        || point_in_rect(pt.x, pt.y, forward)
                        || point_in_rect(pt.x, pt.y, reload)
                        || point_in_rect(pt.x, pt.y, address)
                        || point_in_rect(pt.x, pt.y, min_btn)
                        || point_in_rect(pt.x, pt.y, max_btn)
                        || point_in_rect(pt.x, pt.y, close_btn)
                    {
                        is_interactive = true;
                    }
                });
                if !is_interactive {
                    return LRESULT(WindowsAndMessaging::HTCAPTION as isize);
                }
            }
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_NCCREATE => {
            let _ = l_param.0 as *const CREATESTRUCTW;
            LRESULT(1)
        }
        WM_CREATE => LRESULT(0),
        WindowsAndMessaging::WM_KILLFOCUS => {
            with_app(hwnd, |app| {
                if app.renaming_folder_id.is_some() {
                    app.confirm_rename();
                }
            });
            LRESULT(0)
        }
        WM_SIZE => {
            unsafe {
                let _ = WindowsAndMessaging::SetTimer(Some(hwnd), BACKGROUND_TIMER_ID, 150, None);
            }
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
        WindowsAndMessaging::WM_LBUTTONDBLCLK => {
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
            if w_param.0 == BACKGROUND_TIMER_ID {
                unsafe {
                    let _ = WindowsAndMessaging::KillTimer(Some(hwnd), BACKGROUND_TIMER_ID);
                }
                with_app(hwnd, |app| app.regenerate_background());
                return LRESULT(0);
            }
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
                            app.command_selected_index = None;
                            app.command_scroll_offset = 0;
                            
                            let current_text = get_window_text(app.address_hwnd);
                            if current_text != app.last_address_text {
                                app.last_address_text = current_text.clone();
                                if app.has_typed && !app.is_deleting {
                                    app.try_autofill(&current_text);
                                }
                            }
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
                        if app.rename_selected {
                            app.rename_selected = false;
                            app.rename_buffer.clear();
                        } else {
                            app.rename_buffer.pop();
                        }
                        app.refresh();
                        handled = true;
                    } else if let Some(c) = char::from_u32(ch) {
                        if !c.is_control() {
                            if app.rename_selected {
                                app.rename_selected = false;
                                app.rename_buffer.clear();
                            }
                            app.rename_buffer.push(c);
                            app.refresh();
                            handled = true;
                        }
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
                    } else if key == 0x25 || key == 0x26 || key == 0x27 || key == 0x28 || key == 0x24 || key == 0x23 {
                        if app.rename_selected {
                            app.rename_selected = false;
                            app.refresh();
                        }
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
                if app.renaming_folder_id.is_some() {
                    return;
                }
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
        draw_bitmap_fit(hdc, rect, favicon, false);
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

unsafe fn draw_bitmap_fit(hdc: HDC, rect: RECT, bitmap: &FaviconBitmap, dimmed: bool) {
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
        SourceConstantAlpha: if dimmed { 90 } else { 255 },
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

fn strip_google_transient_params(url: &str) -> String {
    if !url.contains("google.com") {
        return url.to_string();
    }
    if let Some(pos) = url.find('?') {
        let base = &url[..pos];
        let query = &url[pos + 1..];
        let mut clean_params = Vec::new();
        for param in query.split('&') {
            if param.starts_with("zx=") || param.starts_with("no_sw_cr=") {
                continue;
            }
            clean_params.push(param);
        }
        if clean_params.is_empty() {
            base.to_string()
        } else {
            format!("{}?{}", base, clean_params.join("&"))
        }
    } else {
        url.to_string()
    }
}

fn normalize_url_for_dedup(url: &str) -> String {
    let clean = strip_google_transient_params(url);
    let mut normalized = clean.trim().to_ascii_lowercase();
    if let Some(rest) = normalized.strip_prefix("https://") {
        normalized = rest.to_string();
    } else if let Some(rest) = normalized.strip_prefix("http://") {
        normalized = rest.to_string();
    }
    if let Some(rest) = normalized.strip_prefix("www.") {
        normalized = rest.to_string();
    }
    if normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
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

fn percent_decode(input: &str) -> String {
    let mut decoded = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let mut hex = String::new();
            if let Some(h1) = chars.next() { hex.push(h1); }
            if let Some(h2) = chars.next() { hex.push(h2); }
            if let Ok(val) = u8::from_str_radix(&hex, 16) {
                decoded.push(val as char);
            } else {
                decoded.push('%');
                decoded.push_str(&hex);
            }
        } else if ch == '+' {
            decoded.push(' ');
        } else {
            decoded.push(ch);
        }
    }
    decoded
}

fn extract_search_query(url: &str) -> Option<String> {
    if url.contains("google.com/search?") {
        if let Some(pos) = url.find("q=") {
            let query_part = &url[pos + 2..];
            let query_end = query_part.find('&').unwrap_or(query_part.len());
            let encoded_query = &query_part[..query_end];
            let decoded = percent_decode(encoded_query);
            if !decoded.trim().is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

fn label_for_url(url: &str) -> String {
    let clean = strip_google_transient_params(url);
    if let Some(query) = extract_search_query(&clean) {
        return format!("Search: \"{}\"", query);
    }
    let without_scheme = clean
        .strip_prefix("https://")
        .or_else(|| clean.strip_prefix("http://"))
        .unwrap_or(&clean);
    let trimmed = without_scheme.trim_end_matches('/');
    if trimmed.is_empty() {
        return "New Tab".to_string();
    }
    trimmed.to_string()
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
    if let Ok(appdata) = std::env::var("APPDATA") {
        let mut path = PathBuf::from(appdata);
        path.push("Aster");
        let _ = std::fs::create_dir_all(&path);
        path.push(STATE_FILE);
        path
    } else {
        PathBuf::from(STATE_FILE)
    }
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

#[allow(dead_code)]
fn measure_text_width(hdc: HDC, font: &HFONT, text: &str) -> i32 {
    unsafe {
        let old_font = SelectObject(hdc, HGDIOBJ(font.0));
        let wide = to_wide(text);
        let text_len = wide.len().saturating_sub(1);
        let mut size = windows::Win32::Foundation::SIZE { cx: 0, cy: 0 };
        let _ = Gdi::GetTextExtentPoint32W(hdc, &wide[..text_len], &mut size);
        let _ = SelectObject(hdc, old_font);
        size.cx
    }
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

#[allow(dead_code)]
fn is_prefix_match(url: &str, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return false;
    }
    if url.to_ascii_lowercase().starts_with(&query) {
        return true;
    }
    if let Some(q) = extract_search_query(url) {
        if q.to_ascii_lowercase().starts_with(&query) {
            return true;
        }
    }
    clean_all_prefixes(url).to_ascii_lowercase().starts_with(&query)
}

fn clean_all_prefixes(url: &str) -> &str {
    let mut clean = url;
    let strip_prefixes = ["https://www.", "http://www.", "https://", "http://", "www."];
    for pref in &strip_prefixes {
        if clean.to_lowercase().starts_with(pref) {
            clean = &clean[pref.len()..];
            break;
        }
    }
    clean
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_frecency() {
        let now = 10000000;
        
        // Recent visit (< 4 hours ago, e.g. 1 hour ago)
        let score_recent = calculate_frecency(5, now - 3600, now);
        assert_eq!(score_recent, 5 * 100);
        
        // Medium recent visit (12 hours ago)
        let score_medium = calculate_frecency(10, now - 12 * 3600, now);
        assert_eq!(score_medium, 10 * 80);
        
        // Weekly visit (3 days ago)
        let score_weekly = calculate_frecency(20, now - 3 * 24 * 3600, now);
        assert_eq!(score_weekly, 20 * 60);

        // Monthly visit (15 days ago)
        let score_monthly = calculate_frecency(15, now - 15 * 24 * 3600, now);
        assert_eq!(score_monthly, 15 * 30);

        // Old visit (40 days ago)
        let score_old = calculate_frecency(8, now - 40 * 24 * 3600, now);
        assert_eq!(score_old, 8 * 10);
    }

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello+world"), "hello world");
        assert_eq!(percent_decode("rust%20win32%2Bgui"), "rust win32+gui");
    }

    #[test]
    fn test_extract_search_query() {
        assert_eq!(
            extract_search_query("https://www.google.com/search?q=rust+win32+gui&ie=UTF-8"),
            Some("rust win32 gui".to_string())
        );
        assert_eq!(
            extract_search_query("https://google.com/search?q=cat%20videos"),
            Some("cat videos".to_string())
        );
        assert_eq!(
            extract_search_query("https://github.com/microsoft/win32"),
            None
        );
    }

    #[test]
    fn test_normalize_url_for_dedup() {
        assert_eq!(normalize_url_for_dedup("https://google.com/"), "google.com");
        assert_eq!(normalize_url_for_dedup("http://Google.com"), "google.com");
        assert_eq!(normalize_url_for_dedup("https://www.google.com/"), "google.com");
        assert_eq!(normalize_url_for_dedup("  https://www.google.com/search?q=foo/ "), "google.com/search?q=foo");
    }

    #[test]
    fn test_strip_google_transient_params() {
        assert_eq!(
            strip_google_transient_params("https://www.google.com/?zx=1779122310040"),
            "https://www.google.com/"
        );
        assert_eq!(
            strip_google_transient_params("https://www.google.com/search?q=cat&zx=123&no_sw_cr=1"),
            "https://www.google.com/search?q=cat"
        );
        assert_eq!(
            strip_google_transient_params("https://github.com/"),
            "https://github.com/"
        );
    }
}
