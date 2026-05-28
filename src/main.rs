#![windows_subsystem = "windows"]

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fs, mem,
    path::{Path, PathBuf},
    process::Command,
    ptr,
    sync::mpsc,
};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose, Engine as _};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::PWSTR,
    core::*,
    Win32::{
        Foundation::{
            LocalFree, COLORREF, E_POINTER, GENERIC_READ, HINSTANCE, HLOCAL, HWND, LPARAM, LRESULT,
            POINT, RECT, WPARAM,
        },
        Graphics::Dwm::{
            DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_CAPTION_COLOR,
            DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
        },
        Graphics::Gdi::{
            self, AlphaBlend, BeginPaint, BitBlt, CreateBitmap, CreateCompatibleBitmap,
            CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateRectRgn,
            CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint,
            FillRect, GetMonitorInfoW, GetStockObject, InvalidateRect, LineTo, MonitorFromWindow,
            MoveToEx, RoundRect, ScreenToClient, SelectClipRgn, SelectObject, SetBkMode,
            SetTextColor, SetViewportOrgEx, SetWindowRgn, StretchBlt, AC_SRC_ALPHA, AC_SRC_OVER,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, DT_CENTER,
            DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HBITMAP, HBRUSH, HDC, HFONT,
            HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, NULL_BRUSH, NULL_PEN, SRCCOPY,
            TRANSPARENT,
        },
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
        },
        Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB},
        System::{Com::*, Com::Urlmon::URLDownloadToCacheFileW, LibraryLoader},
        UI::{
            Controls::{EM_SETMARGINS, EM_SETSEL, MARGINS},
            HiDpi,
            Input::KeyboardAndMouse::{
                GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_ESCAPE, VK_F11,
                VK_F5, VK_MENU, VK_RETURN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                self, CreateIconIndirect, GetCursorPos, GetTopWindow, GetWindow, CREATESTRUCTW,
                CW_USEDEFAULT, EC_LEFTMARGIN, EC_RIGHTMARGIN, GWLP_USERDATA, GWLP_WNDPROC,
                GWL_STYLE, GW_HWNDNEXT, HICON, HMENU, HWND_TOP, ICONINFO, ICON_BIG, ICON_SMALL,
                IDC_ARROW, MSG, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP,
                WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
                WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT,
                WM_RBUTTONDOWN, WM_SETCURSOR, WM_SETFOCUS, WM_SETFONT, WM_SETICON, WM_SIZE,
                WM_TIMER, WNDCLASSW, WNDPROC, WS_CHILD, WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW,
                WS_POPUP, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
};

const APP_NAME: PCWSTR = w!("Aster");
const CLASS_NAME: PCWSTR = w!("AsterWindow");
const ADDRESS_ID: i32 = 1001;
const COMMAND_POPUP_ID: i32 = 1002;
const DOWNLOAD_POPUP_ID: i32 = 1003;
const FIND_ID: i32 = 1004;
const BOOKMARK_POPUP_ID: i32 = 1005;
const DEFAULT_URL: &str = "https://www.google.com";
const SIDEBAR_EXPANDED: f32 = 248.0;
const SIDEBAR_HIDDEN: f32 = 0.0;
const TOPBAR_EXPANDED: f32 = 58.0;
const TOPBAR_HIDDEN: f32 = 0.0;
const HOVER_ZONE: i32 = 4;
const TOPBAR_HEIGHT: i32 = 58;
const SIDEBAR_TIMER_ID: usize = 42;
const HOVER_LEAVE_TIMER_ID: usize = 43;
const HOVER_DETECT_TIMER_ID: usize = 44;
const BACKGROUND_TIMER_ID: usize = 45;
const LOADING_TIMER_ID: usize = 46;
const TOPBAR_TIMER_ID: usize = 47;
const DOWNLOAD_TIMER_ID: usize = 48;
const SPLIT_MAX_PANES: usize = 4;
const SPLIT_GAP: i32 = 6;
const STATE_FILE: &str = ".aster-state";
const FOCUS_EDIT_MSG: u32 = WM_APP + 1;
const FAVICON_FETCHED_MSG: u32 = WM_APP + 2;
const WM_COPYDATA: u32 = 0x004A;

#[allow(non_snake_case)]
#[repr(C)]
struct COPYDATASTRUCT {
    pub dwData: usize,
    pub cbData: u32,
    pub lpData: *mut std::ffi::c_void,
}

thread_local! {
    static WITH_APP_GUARD: Cell<bool> = const { Cell::new(false) };
}
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
const MENU_TAB_DUPLICATE: usize = 3508;
const MENU_HISTORY_BASE: usize = 3600;
const MENU_REOPEN_CLOSED: usize = 3700;
const MENU_RECENTLY_CLOSED_BASE: usize = 3710;
const MENU_ADDRESS_BOOKMARK: usize = 3800;
const MENU_ADDRESS_BOOKMARKS: usize = 3801;
const MENU_ADDRESS_ZOOM_OUT: usize = 3802;
const MENU_ADDRESS_ZOOM_RESET: usize = 3803;
const MENU_ADDRESS_ZOOM_IN: usize = 3804;
const MENU_ADDRESS_CLEAR_RELOAD: usize = 3805;
const MENU_BOOKMARK_OPEN_BASE: usize = 3900;
const MENU_TAB_SPLIT_WITH_ACTIVE: usize = 4000;
const MENU_TAB_SPLIT_NEXT: usize = 4001;
const MENU_TAB_UNSPLIT: usize = 4002;
const MENU_TAB_SPLIT_HORIZONTAL: usize = 4003;
const MENU_TAB_SPLIT_VERTICAL: usize = 4004;
const MENU_TAB_SPLIT_GRID: usize = 4005;
const MENU_EXTENSION_MANAGE: usize = 4100;
const MENU_EXTENSION_STORE: usize = 4101;
const MENU_EXTENSION_REFRESH: usize = 4102;
const MENU_EXTENSION_ITEM_BASE: usize = 4110;
const MENU_WIDTH: i32 = 270;
const MENU_ROW_HEIGHT: i32 = 34;
const CHROME_WEB_STORE_URL: &str = "https://chromewebstore.google.com/";

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
static mut OLD_FIND_PROC: WNDPROC = None;
static mut OLD_COMMAND_POPUP_PROC: WNDPROC = None;
static mut OLD_RENAME_EDIT_PROC: WNDPROC = None;
static mut OLD_OVERLAY_MENU_PROC: WNDPROC = None;
static mut OLD_DOWNLOAD_POPUP_PROC: WNDPROC = None;
static mut OLD_BOOKMARK_POPUP_PROC: WNDPROC = None;
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

struct FaviconFetchResult {
    host: String,
    path: String,
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

struct PaintCache {
    bitmap: HBITMAP,
    dc: HDC,
    width: i32,
    height: i32,
    old_bitmap: HGDIOBJ,
}

impl Drop for PaintCache {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.old_bitmap);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
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
    sidebar_order: u64,
}

struct Tab {
    id: usize,
    workspace_id: usize,
    folder_id: Option<usize>,
    pinned: bool,
    pinned_url: Option<String>,
    split_group_id: Option<usize>,
    sidebar_order: u64,
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
    is_sleeping: bool,
    is_loading: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitLayout {
    Horizontal,
    Vertical,
    Grid,
}

struct SplitGroup {
    id: usize,
    workspace_id: usize,
    tab_ids: Vec<usize>,
    layout: SplitLayout,
    active_tab_id: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitDropZone {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SplitDropTarget {
    target_tab_id: usize,
    zone: SplitDropZone,
}

struct ClosedTab {
    url: String,
    title: String,
    workspace_id: usize,
    folder_id: Option<usize>,
}

struct BookmarkFolder {
    id: usize,
    parent_id: Option<usize>,
    name: String,
    sidebar_order: u64,
}

#[derive(Clone)]
struct Bookmark {
    id: usize,
    folder_id: Option<usize>,
    title: String,
    url: String,
    tags: Vec<String>,
    created_at: u64,
    sidebar_order: u64,
}

#[derive(Clone)]
struct HistoryEntry {
    title: String,
    url: String,
}

#[derive(Clone, Debug)]
struct VisitedSite {
    title: String,
    url: String,
    visit_count: u32,
    last_visit_time: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HistorySortMode {
    Latest,
    Oldest,
    MostVisited,
}

impl HistorySortMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Oldest => "oldest",
            Self::MostVisited => "most_visited",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "oldest" => Self::Oldest,
            "most_visited" => Self::MostVisited,
            _ => Self::Latest,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct PortableBookmark {
    title: String,
    url: String,
    #[serde(default)]
    folder: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    created_at: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct PortableHistoryEntry {
    title: String,
    url: String,
    #[serde(default = "default_visit_count")]
    visit_count: u32,
    #[serde(default)]
    last_visit_time: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct PortableCookie {
    name: String,
    value: String,
    domain: String,
    #[serde(default = "default_cookie_path")]
    path: String,
    #[serde(default)]
    expires: Option<f64>,
    #[serde(default)]
    secure: bool,
    #[serde(default)]
    http_only: bool,
    #[serde(default)]
    same_site: String,
}

#[derive(Default, Serialize, Deserialize)]
struct AsterDataExport {
    #[serde(default)]
    app: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    exported_at: u64,
    #[serde(default)]
    bookmarks: Vec<PortableBookmark>,
    #[serde(default)]
    history: Vec<PortableHistoryEntry>,
    #[serde(default)]
    cookies: Vec<PortableCookie>,
}

#[derive(Deserialize)]
struct ImportUploadFile {
    name: String,
    #[serde(default)]
    path: String,
    content: String,
}

#[derive(Default)]
struct ImportStats {
    bookmarks: usize,
    history: usize,
    cookies: usize,
    accounts: usize,
    skipped: usize,
    errors: Vec<String>,
}

impl ImportStats {
    fn absorb(&mut self, other: ImportStats) {
        self.bookmarks += other.bookmarks;
        self.history += other.history;
        self.cookies += other.cookies;
        self.accounts += other.accounts;
        self.skipped += other.skipped;
        self.errors.extend(other.errors);
    }

    fn message(&self) -> String {
        let mut parts = Vec::new();
        if self.bookmarks > 0 {
            parts.push(format!("{} bookmarks", self.bookmarks));
        }
        if self.history > 0 {
            parts.push(format!("{} history entries", self.history));
        }
        if self.cookies > 0 {
            parts.push(format!("{} session cookies", self.cookies));
        }
        if self.accounts > 0 {
            parts.push(format!("{} account cookies", self.accounts));
        }
        if parts.is_empty() && self.errors.is_empty() {
            parts.push("No new data found".to_string());
        }
        if self.skipped > 0 {
            parts.push(format!("{} duplicates skipped", self.skipped));
        }
        if !self.errors.is_empty() {
            parts.push(format!("{} files need attention", self.errors.len()));
        }
        format!("Import complete: {}.", parts.join(", "))
    }
}

#[derive(Clone)]
struct BrowserExtensionInfo {
    id: String,
    name: String,
    enabled: bool,
}

fn default_visit_count() -> u32 {
    1
}

fn default_cookie_path() -> String {
    "/".to_string()
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
    AddressMenu,
    Bookmarks,
    Extensions,
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
    SplitGroup(usize),
    Tab(usize),
    TabGhost(usize),
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
    AddressMenu,
    Back,
    Forward,
    Reload,
    Settings,
    HistoryPage,
    ExtensionStore,
    ExtensionsPage,
    SettingsPage,
    ModeRow,
    ModeAuto,
    ModeDark,
    ModeLight,
    DownloadIndicator(usize),
    DownloadOverflow,
    DownloadCancel(usize),
    DownloadPause(usize),
    DownloadOpen(usize),
    ExtensionsButton,
    FindPrev,
    FindNext,
    FindClose,
    SplitLayout,
    SplitUnsplit,
    MinButton,
    MaxButton,
    CloseButton,
    DefaultBubbleClose,
    DefaultBubbleSetDefault,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DownloadPanelMode {
    Single(usize),
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DownloadAction {
    TogglePause(usize),
    Cancel(usize),
    ShowInFolder(usize),
    Delete(usize),
}

struct DownloadItem {
    id: usize,
    file_name: String,
    file_path: String,
    uri: String,
    received_bytes: i64,
    total_bytes: i64,
    state: COREWEBVIEW2_DOWNLOAD_STATE,
    paused: bool,
    completed_at: Option<std::time::Instant>,
    cancelled_at: Option<std::time::Instant>,
    operation: Option<ICoreWebView2DownloadOperation>,
}

struct DownloadSnapshot {
    uri: String,
    file_path: String,
    received_bytes: i64,
    total_bytes: i64,
    state: COREWEBVIEW2_DOWNLOAD_STATE,
}

struct DownloadToastState {
    start_time: std::time::Instant,
    fading: bool,
    slide_x: f32,
}

struct BookmarkToastState {
    start_time: std::time::Instant,
    is_unbookmark: bool,
}

struct DownloadCollapseAnim {
    start_time: std::time::Instant,
    duration: u64,
}

struct DownloadRemovalAnim {
    start_time: std::time::Instant,
    duration: u64,
    removed_id: usize,
    removed_index: usize,
    old_count: usize,
    removed_progress: f32,
    removed_completed: bool,
    removed_completed_at: Option<std::time::Instant>,
    removed_cancelled: bool,
    removed_cancelled_at: Option<std::time::Instant>,
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

#[derive(Clone, Copy, PartialEq)]
enum StartupMode {
    HomePage,
    LastSession,
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
    nav_icon: HFONT,
    url: HFONT,
}

impl Drop for UiFonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.body.0));
            let _ = DeleteObject(HGDIOBJ(self.small.0));
            let _ = DeleteObject(HGDIOBJ(self.icon.0));
            let _ = DeleteObject(HGDIOBJ(self.toolbar_icon.0));
            let _ = DeleteObject(HGDIOBJ(self.nav_icon.0));
            let _ = DeleteObject(HGDIOBJ(self.url.0));
        }
    }
}

struct UiBrushes {
    black: HBRUSH,
    panel: HBRUSH,
    secondary: HBRUSH,
    panel_2: HBRUSH,
    edit: HBRUSH,
    hover: HBRUSH,
}

impl Drop for UiBrushes {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.black.0));
            let _ = DeleteObject(HGDIOBJ(self.panel.0));
            let _ = DeleteObject(HGDIOBJ(self.secondary.0));
            let _ = DeleteObject(HGDIOBJ(self.panel_2.0));
            let _ = DeleteObject(HGDIOBJ(self.edit.0));
            let _ = DeleteObject(HGDIOBJ(self.hover.0));
        }
    }
}

struct App {
    hwnd: HWND,
    address_hwnd: HWND,
    find_hwnd: HWND,
    command_hwnd: HWND,
    overlay_menu_hwnd: HWND,
    bookmark_popup_hwnd: HWND,
    environment: ICoreWebView2Environment,
    workspaces: Vec<Workspace>,
    folders: Vec<Folder>,
    bookmark_folders: Vec<BookmarkFolder>,
    bookmarks: Vec<Bookmark>,
    tabs: Vec<Tab>,
    split_groups: Vec<SplitGroup>,
    favicon_cache: HashMap<String, FaviconBitmap>,
    pending_favicon_hosts: Vec<String>,
    extensions: Vec<BrowserExtensionInfo>,
    extension_notice: Option<String>,
    active_workspace: usize,
    active: usize,
    next_id: usize,
    next_split_group_id: usize,
    next_workspace_id: usize,
    next_folder_id: usize,
    next_bookmark_id: usize,
    next_bookmark_folder_id: usize,
    next_sidebar_order: u64,
    workspace_active_tabs: Vec<(usize, usize)>,
    sidebar_scroll_offset: usize,
    workspace_swipe_accum: i32,
    last_workspace_swipe: Option<std::time::Instant>,
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
    topbar_height: f32,
    topbar_target: f32,
    topbar_mode: SidebarMode,
    topbar_expand_mode: SidebarMode,
    animating_topbar: bool,
    hovering_topbar: bool,
    last_clip_width: Cell<f32>,
    last_clip_top: Cell<f32>,
    last_bounds_rect: Cell<RECT>,
    dominant_color: u32,
    secondary_color: u32,
    accent_color: u32,
    custom_keybinds: Vec<(String, String)>,
    site_mode: SiteMode,
    history_sort_mode: HistorySortMode,
    history_visit_sort_desc: bool,
    import_export_notice: Option<String>,
    startup_mode: StartupMode,
    settings_open: bool,
    mode_menu_open: bool,
    overlay_menu: Option<OverlayMenu>,
    drag_state: Option<DragState>,
    drag_ghost: RefCell<Option<DragGhost>>,
    drop_target: Option<DropTarget>,
    split_drop_target: Option<SplitDropTarget>,
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
    find_open: bool,
    find_query: String,
    find_match_count: usize,
    find_current_match: usize,
    is_deleting: bool,
    last_address_text: String,
    has_typed: bool,
    closed_tabs: Vec<ClosedTab>,
    downloads: Vec<DownloadItem>,
    next_download_id: usize,
    download_toast: Option<DownloadToastState>,
    download_panel: Option<DownloadPanelMode>,
    download_panel_reveal: f32,
    download_panel_reveal_target: f32,
    download_popup_hwnd: HWND,
    bookmark_toast: Option<BookmarkToastState>,
    download_removal_anim: Option<DownloadRemovalAnim>,
    download_collapse_anim: Option<DownloadCollapseAnim>,
    paint_cache: RefCell<Option<PaintCache>>,
    dl_panel_cache: RefCell<Option<PaintCache>>,
    show_default_bubble: bool,
    default_bubble_dismissed: bool,
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
#[allow(dead_code)]
enum DropTarget {
    PinnedSection,
    UnpinnedSection,
    RootAfter {
        pinned: bool,
        row: Option<SidebarRow>,
    },
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
            nav_icon: create_font_with_face(18, 400, w!("Segoe Fluent Icons"))?,
            url: create_font_with_face(13, 400, w!("Segoe UI Variable Text"))?,
        };
        let brushes = UiBrushes {
            black: solid_brush(COLOR_BLACK),
            panel: solid_brush(COLOR_PANEL),
            secondary: solid_brush(COLOR_PANEL),
            panel_2: solid_brush(COLOR_PANEL_2),
            edit: solid_brush(0x080808),
            hover: solid_brush(COLOR_SURFACE_HOVER),
        };

        let address_hwnd = create_address_bar(hwnd)?;
        let find_hwnd = create_find_edit(hwnd)?;
        let command_hwnd = create_command_popup(hwnd)?;
        let overlay_menu_hwnd = create_overlay_menu(hwnd)?;
        let download_popup_hwnd = create_download_popup(hwnd)?;
        let bookmark_popup_hwnd = create_bookmark_popup(hwnd)?;
        unsafe {
            let _ = WindowsAndMessaging::SendMessageW(
                address_hwnd,
                WM_SETFONT,
                Some(WPARAM(fonts.url.0 as usize)),
                Some(LPARAM(1)),
            );
            let _ = WindowsAndMessaging::SendMessageW(
                find_hwnd,
                WM_SETFONT,
                Some(WPARAM(fonts.url.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        let mut app = Self {
            hwnd,
            address_hwnd,
            find_hwnd,
            command_hwnd,
            overlay_menu_hwnd,
            bookmark_popup_hwnd,
            environment,
            workspaces: vec![Workspace {
                id: 1,
                name: "Space 1".to_string(),
            }],
            folders: Vec::new(),
            bookmark_folders: Vec::new(),
            bookmarks: Vec::new(),
            tabs: Vec::new(),
            split_groups: Vec::new(),
            favicon_cache: HashMap::new(),
            pending_favicon_hosts: Vec::new(),
            extensions: Vec::new(),
            extension_notice: None,
            active_workspace: 1,
            active: 0,
            next_id: 1,
            next_split_group_id: 1,
            next_workspace_id: 2,
            next_folder_id: 1,
            next_bookmark_id: 1,
            next_bookmark_folder_id: 1,
            next_sidebar_order: 1024,
            workspace_active_tabs: Vec::new(),
            sidebar_scroll_offset: 0,
            workspace_swipe_accum: 0,
            last_workspace_swipe: None,
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
            topbar_height: TOPBAR_HIDDEN,
            topbar_target: TOPBAR_HIDDEN,
            topbar_mode: SidebarMode::Hidden,
            topbar_expand_mode: SidebarMode::Hidden,
            animating_topbar: false,
            hovering_topbar: false,
            last_clip_width: Cell::new(0.0),
            last_clip_top: Cell::new(0.0),
            last_bounds_rect: Cell::new(RECT {
                left: -1,
                top: -1,
                right: -1,
                bottom: -1,
            }),
            dominant_color: COLOR_BLACK,
            secondary_color: COLOR_PANEL,
            accent_color: COLOR_ACCENT,
            custom_keybinds: Vec::new(),
            site_mode: SiteMode::Auto,
            history_sort_mode: HistorySortMode::Latest,
            history_visit_sort_desc: true,
            import_export_notice: None,
            startup_mode: StartupMode::LastSession,
            settings_open: false,
            mode_menu_open: false,
            overlay_menu: None,
            drag_state: None,
            drag_ghost: RefCell::new(None),
            drop_target: Some(DropTarget::None),
            split_drop_target: None,
            background_cache: RefCell::new(None),
            visited_sites: Vec::new(),
            closed_tabs: Vec::new(),
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
            find_open: false,
            find_query: String::new(),
            find_match_count: 0,
            find_current_match: 0,
            is_deleting: false,
            last_address_text: String::new(),
            has_typed: false,
            downloads: Vec::new(),
            next_download_id: 1,
            download_toast: None,
            download_panel: None,
            download_panel_reveal: 0.0,
            download_panel_reveal_target: 0.0,
            download_popup_hwnd,
            bookmark_toast: None,
            download_removal_anim: None,
            download_collapse_anim: None,
            paint_cache: RefCell::new(None),
            dl_panel_cache: RefCell::new(None),
            show_default_bubble: !is_aster_default_browser(),
            default_bubble_dismissed: false,
        };
        app.load_state()?;
        app.default_bubble_dismissed = false;
        app.show_default_bubble = !is_aster_default_browser();
        app.save_state();

        let args: Vec<String> = std::env::args().collect();
        let mut startup_url = None;
        if args.len() > 1 {
            for arg in args.iter().skip(1) {
                if !arg.starts_with('-') && !arg.starts_with('/') {
                    startup_url = Some(normalize_address(arg));
                    break;
                }
            }
        }

        if let Some(url) = startup_url {
            let _ = app.create_tab(&url);
            if let Some(index) = app.tabs.iter().position(|t| t.url == url) {
                app.switch_to(index, true);
            }
        } else if app.startup_mode == StartupMode::LastSession {
            if let Some(index) = app.active_tab_index() {
                app.switch_to(index, true);
            }
        }

        if app.tabs.is_empty() {
            let _ = app.create_tab(DEFAULT_URL);
            if let Some(index) = app.active_tab_index() {
                app.switch_to(index, true);
            }
        }

        app.ensure_default_bookmark_folder();
        unsafe {
            let _ = WindowsAndMessaging::SetTimer(Some(app.hwnd), HOVER_DETECT_TIMER_ID, 100, None);
        }
        Ok(app)
    }

    fn create_tab(&mut self, url: &str) -> AppResult<()> {
        self.create_tab_in_workspace(url, self.active_workspace, None, false, true, None)
    }

    fn allocate_sidebar_order(&mut self) -> u64 {
        let order = self.next_sidebar_order;
        self.next_sidebar_order = self.next_sidebar_order.saturating_add(1024);
        order
    }

    fn ensure_default_bookmark_folder(&mut self) {
        if self.bookmark_folders.is_empty() {
            self.bookmark_folders.push(BookmarkFolder {
                id: 1,
                parent_id: None,
                name: "Favorites".to_string(),
                sidebar_order: 1024,
            });
            self.next_bookmark_folder_id = 2;
        }
    }

    fn active_page_snapshot(&self) -> Option<(String, String)> {
        self.active_tab_index().and_then(|index| {
            self.tabs.get(index).and_then(|tab| {
                if tab.url.trim().is_empty() || tab.url == "about:blank" {
                    None
                } else {
                    Some((tab.title.clone(), tab.url.clone()))
                }
            })
        })
    }

    fn bookmark_index_for_url(&self, url: &str) -> Option<usize> {
        let normalized = normalize_url_for_dedup(url);
        self.bookmarks
            .iter()
            .position(|bookmark| normalize_url_for_dedup(&bookmark.url) == normalized)
    }

    fn is_active_bookmarked(&self) -> bool {
        self.active_page_snapshot()
            .and_then(|(_, url)| self.bookmark_index_for_url(&url))
            .is_some()
    }

    fn toggle_active_bookmark(&mut self) {
        let Some((title, url)) = self.active_page_snapshot() else {
            return;
        };
        let mut added = false;
        if let Some(index) = self.bookmark_index_for_url(&url) {
            self.bookmarks.remove(index);
        } else {
            self.ensure_default_bookmark_folder();
            let folder_id = self.bookmark_folders.first().map(|folder| folder.id);
            let id = self.next_bookmark_id;
            self.next_bookmark_id += 1;
            let order = self.allocate_sidebar_order();
            let host_tag = display_host(&url);
            let tags = if host_tag.is_empty() {
                Vec::new()
            } else {
                vec![host_tag]
            };
            self.bookmarks.push(Bookmark {
                id,
                folder_id,
                title: if title.trim().is_empty() {
                    label_for_url(&url)
                } else {
                    title
                },
                url,
                tags,
                created_at: current_timestamp(),
                sidebar_order: order,
            });
            added = true;
        }
        self.save_state();
        if added {
            self.show_bookmark_toast();
            self.refresh();
        } else {
            self.show_unbookmark_toast();
            self.refresh();
        }
    }

    fn address_menu_rect(&self) -> RECT {
        let pill = self.address_pill_rect();
        RECT {
            left: pill.right - 34,
            top: pill.top + 3,
            right: pill.right - 6,
            bottom: pill.bottom - 3,
        }
    }

    fn find_bar_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let pushed_top = self.topbar_pushed_height();
        let y = if self.topbar_mode == SidebarMode::Pushed
            || self.topbar_expand_mode == SidebarMode::Pushed
        {
            self.topbar_y() + 8
        } else {
            pushed_top + 72
        };
        let right = (rect.right - 150).max(360);
        RECT {
            left: (right - 360).max(160),
            top: y,
            right,
            bottom: y + 42,
        }
    }

    fn find_input_rect(&self) -> RECT {
        let bar = self.find_bar_rect();
        RECT {
            left: bar.left + 14,
            top: bar.top + 10,
            right: bar.right - 178,
            bottom: bar.bottom - 10,
        }
    }

    fn find_prev_rect(&self) -> RECT {
        let bar = self.find_bar_rect();
        RECT {
            left: bar.right - 114,
            top: bar.top + 7,
            right: bar.right - 88,
            bottom: bar.bottom - 7,
        }
    }

    fn find_next_rect(&self) -> RECT {
        let prev = self.find_prev_rect();
        RECT {
            left: prev.right + 4,
            top: prev.top,
            right: prev.right + 30,
            bottom: prev.bottom,
        }
    }

    fn find_close_rect(&self) -> RECT {
        let next = self.find_next_rect();
        RECT {
            left: next.right + 8,
            top: next.top,
            right: next.right + 34,
            bottom: next.bottom,
        }
    }

    fn open_find_bar(&mut self) {
        self.find_open = true;
        set_window_text(self.find_hwnd, &self.find_query);
        self.layout();
        unsafe {
            let _ = SetFocus(Some(self.find_hwnd));
            let _ = WindowsAndMessaging::SendMessageW(
                self.find_hwnd,
                EM_SETSEL,
                Some(WPARAM(0)),
                Some(LPARAM(-1)),
            );
        }
        self.run_find_script(0);
        self.refresh();
    }

    fn close_find_bar(&mut self) {
        self.find_open = false;
        self.find_query.clear();
        self.find_match_count = 0;
        self.find_current_match = 0;
        self.run_find_script(0);
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

    fn hide_find_bar(&mut self) {
        self.find_open = false;
        self.find_match_count = 0;
        self.find_current_match = 0;
        self.run_find_script(0);
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

    fn run_find_script(&mut self, delta: i32) {
        if !self.find_open && self.find_query.is_empty() {
            self.execute_find_script("", 0);
            return;
        }
        let query = self.find_query.clone();
        self.execute_find_script(&query, delta);
    }

    fn execute_find_script(&self, query: &str, delta: i32) {
        let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        else {
            return;
        };
        let script = build_find_script(query, delta, colorref_to_css(self.accent_color));
        let hwnd = self.hwnd;
        unsafe {
            let js = CoTaskMemPWSTR::from(script.as_str());
            let _ = tab.webview.ExecuteScript(
                *js.as_ref().as_pcwstr(),
                &ExecuteScriptCompletedHandler::create(Box::new(move |error_code, result| {
                    if error_code.is_ok() {
                        let raw = result.to_string();
                        let count = parse_json_usize_field(&raw, "count").unwrap_or(0);
                        let index = parse_json_usize_field(&raw, "index").unwrap_or(0);
                        with_app(hwnd, |app| {
                            app.find_match_count = count;
                            app.find_current_match = if count == 0 { 0 } else { index + 1 };
                            app.refresh();
                        });
                    }
                    Ok(())
                })),
            );
        }
    }

    fn active_zoom_percent(&self) -> i32 {
        self.active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| {
                let mut factor = 1.0;
                unsafe { tab.controller.ZoomFactor(&mut factor).ok()? };
                Some((factor * 100.0).round() as i32)
            })
            .unwrap_or(100)
    }

    fn set_active_zoom(&mut self, factor: f64) {
        if let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        {
            let factor = factor.clamp(0.25, 5.0);
            unsafe {
                let _ = tab.controller.SetZoomFactor(factor);
            }
            self.open_zoom_menu();
            self.refresh();
        }
    }

    fn adjust_active_zoom(&mut self, delta: f64) {
        let current = self.active_zoom_percent() as f64 / 100.0;
        self.set_active_zoom(current + delta);
    }

    fn reset_active_zoom(&mut self) {
        self.set_active_zoom(1.0);
    }

    fn show_bookmark_toast(&mut self) {
        self.bookmark_toast = Some(BookmarkToastState {
            start_time: std::time::Instant::now(),
            is_unbookmark: false,
        });
        let rect = client_rect(self.hwnd);
        unsafe {
            let _ = WindowsAndMessaging::SetWindowPos(
                self.bookmark_popup_hwnd,
                Some(HWND_TOP),
                18,
                rect.bottom - 74,
                210,
                48,
                WindowsAndMessaging::SWP_NOACTIVATE | WindowsAndMessaging::SWP_SHOWWINDOW,
            );
        }
        self.ensure_download_timer();
    }

    fn show_unbookmark_toast(&mut self) {
        self.bookmark_toast = Some(BookmarkToastState {
            start_time: std::time::Instant::now(),
            is_unbookmark: true,
        });
        let rect = client_rect(self.hwnd);
        unsafe {
            let _ = WindowsAndMessaging::SetWindowPos(
                self.bookmark_popup_hwnd,
                Some(HWND_TOP),
                18,
                rect.bottom - 74,
                210,
                48,
                WindowsAndMessaging::SWP_NOACTIVATE | WindowsAndMessaging::SWP_SHOWWINDOW,
            );
        }
        self.ensure_download_timer();
    }

    fn tick_bookmark_toast(&mut self) {
        if let Some(toast) = &self.bookmark_toast {
            if toast.start_time.elapsed().as_millis() >= 2200 {
                self.bookmark_toast = None;
                unsafe {
                    let _ = WindowsAndMessaging::ShowWindow(
                        self.bookmark_popup_hwnd,
                        WindowsAndMessaging::SW_HIDE,
                    );
                }
            } else {
                unsafe {
                    let _ = InvalidateRect(Some(self.bookmark_popup_hwnd), None, false);
                }
            }
        }
    }

    fn open_address_menu(&mut self, x: i32, y: i32) {
        let bookmark_label = if self.is_active_bookmarked() {
            "Remove Bookmark"
        } else {
            "Bookmark Site"
        };
        self.open_overlay_menu(
            x,
            y,
            MenuTarget::AddressMenu,
            vec![
                menu_item(MENU_ADDRESS_BOOKMARK, bookmark_label),
                menu_item(MENU_ADDRESS_BOOKMARKS, "Show Bookmarks"),
                menu_item(MENU_ADDRESS_ZOOM_OUT, "Zoom Out"),
                menu_item(
                    MENU_ADDRESS_ZOOM_RESET,
                    &format!("Reset Zoom ({}%)", self.active_zoom_percent()),
                ),
                menu_item(MENU_ADDRESS_ZOOM_IN, "Zoom In"),
                menu_item(MENU_ADDRESS_CLEAR_RELOAD, "Clear Cookies/Cache and Reload"),
            ],
        );
    }

    fn open_zoom_menu(&mut self) {
        let rect = client_rect(self.hwnd);
        let y = self.topbar_pushed_height() + 58;
        self.open_overlay_menu(
            rect.right - MENU_WIDTH - 18,
            y,
            MenuTarget::AddressMenu,
            vec![
                menu_item(MENU_ADDRESS_ZOOM_OUT, "Zoom Out"),
                menu_item(
                    MENU_ADDRESS_ZOOM_RESET,
                    &format!("{}%", self.active_zoom_percent()),
                ),
                menu_item(MENU_ADDRESS_ZOOM_IN, "Zoom In"),
            ],
        );
    }

    fn open_bookmarks_menu(&mut self, x: i32, y: i32) {
        let mut items = Vec::new();
        if self.bookmarks.is_empty() {
            items.push(menu_item(MENU_ADDRESS_BOOKMARK, "No Bookmarks Yet"));
        } else {
            for (offset, bookmark) in self.bookmarks.iter().take(20).enumerate() {
                let mut item = menu_item(MENU_BOOKMARK_OPEN_BASE + offset, &bookmark.title);
                item.sublabel = bookmark.url.clone();
                items.push(item);
            }
        }
        self.open_overlay_menu(x, y, MenuTarget::Bookmarks, items);
    }

    fn open_extension_menu(&mut self) {
        self.refresh_extensions();
        let rect = self.extension_button_rect();
        let mut items = vec![
            menu_item(MENU_EXTENSION_MANAGE, "Manage Extensions"),
            menu_item(MENU_EXTENSION_STORE, "Chrome Web Store"),
            menu_item(MENU_EXTENSION_REFRESH, "Refresh Extensions"),
        ];
        for (offset, extension) in self.extensions.iter().take(12).enumerate() {
            let state = if extension.enabled { "Enabled" } else { "Disabled" };
            items.push(menu_item_with_subtitle(
                MENU_EXTENSION_ITEM_BASE + offset,
                &extension.name,
                state,
            ));
        }
        self.open_overlay_menu(rect.right - MENU_WIDTH, rect.bottom + 8, MenuTarget::Extensions, items);
    }

    fn clear_site_data_and_reload(&mut self) {
        if let Some(tab) = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
        {
            let script = r#"(async function() {
  try {
    document.cookie.split(";").forEach((cookie) => {
      const name = cookie.split("=")[0].trim();
      if (name) document.cookie = name + "=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/";
    });
    localStorage.clear();
    sessionStorage.clear();
    if ("caches" in window) {
      const keys = await caches.keys();
      await Promise.all(keys.map((key) => caches.delete(key)));
    }
  } catch (_) {}
  location.reload();
})();"#;
            unsafe {
                let js = CoTaskMemPWSTR::from(script);
                let _ = tab.webview.ExecuteScript(
                    *js.as_ref().as_pcwstr(),
                    &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
                );
            }
        }
    }

    fn attach_web_message_handler(&self, webview: &ICoreWebView2) {
        let hwnd = self.hwnd;
        unsafe {
            let mut token = 0;
            let _ = webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut raw = PWSTR::null();
                        if args.TryGetWebMessageAsString(&mut raw).is_ok() {
                            let message = take_pwstr(raw);
                            with_app(hwnd, |app| app.handle_settings_message(&message));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            );
        }
    }

    fn recreate_secondary_brush(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.brushes.secondary.0));
        }
        self.brushes.secondary = solid_brush(self.secondary_color);
    }

    fn handle_settings_message(&mut self, message: &str) {
        if let Some(value) = message.strip_prefix("settings:accent:") {
            if let Some(color) = parse_css_color_to_colorref(value) {
                self.accent_color = color;
                self.run_find_script(0);
            }
        } else if let Some(value) = message.strip_prefix("settings:dominant:") {
            if let Some(color) = parse_css_color_to_colorref(value) {
                self.dominant_color = color;
            }
        } else if let Some(value) = message.strip_prefix("settings:secondary:") {
            if let Some(color) = parse_css_color_to_colorref(value) {
                self.secondary_color = color;
                self.recreate_secondary_brush();
            }
        } else if message == "settings:open-state-file" {
            let path = state_path();
            let _ = std::process::Command::new("notepad")
                .arg(path.to_string_lossy().as_ref())
                .spawn();
        } else if message == "settings:make-default" {
            make_aster_default_browser();
        } else if let Some(value) = message.strip_prefix("settings:site-mode:") {
            match value {
                "auto" => self.set_site_mode(SiteMode::Auto),
                "dark" => self.set_site_mode(SiteMode::Dark),
                "light" => self.set_site_mode(SiteMode::Light),
                _ => {}
            }
        } else if let Some(value) = message.strip_prefix("settings:startup:") {
            match value {
                "home" => self.startup_mode = StartupMode::HomePage,
                "last" => self.startup_mode = StartupMode::LastSession,
                _ => {}
            }
        } else if let Some(value) = message.strip_prefix("settings:keybind:") {
            if let Some((action, combo)) = value.rsplit_once(':') {
                if !action.trim().is_empty() && !combo.trim().is_empty() {
                    if let Some(existing) = self
                        .custom_keybinds
                        .iter_mut()
                        .find(|(name, _)| name == action)
                    {
                        existing.1 = combo.to_string();
                    } else {
                        self.custom_keybinds
                            .push((action.to_string(), combo.to_string()));
                    }
                }
            }
        } else if message == "settings:export:aster" {
            self.send_settings_export("aster");
        } else if message == "settings:export:bookmarks" {
            self.send_settings_export("bookmarks");
        } else if let Some(raw) = message.strip_prefix("settings:import-files:") {
            let stats = self.import_uploaded_files(raw);
            self.import_export_notice = Some(stats.message());
            self.reload_settings_pages();
        } else if message == "settings:clear-import-notice" {
            self.import_export_notice = None;
            self.reload_settings_pages();
        } else if message == "extensions:refresh" {
            self.extension_notice = Some("Refreshing extensions...".to_string());
            self.refresh_extensions();
        } else if message == "extensions:open-store" {
            self.open_extension_store();
        } else if let Some(path) = message.strip_prefix("extensions:add:") {
            self.add_extension_from_path(&percent_decode(path));
        } else if let Some(raw) = message.strip_prefix("extensions:enable:") {
            if let Some((id, value)) = raw.rsplit_once(':') {
                self.set_extension_enabled(&percent_decode(id), value == "1");
            }
        } else if let Some(id) = message.strip_prefix("extensions:remove:") {
            self.remove_extension(&percent_decode(id));
        } else if let Some(value) = message.strip_prefix("history:sort:") {
            self.history_sort_mode = HistorySortMode::from_str(value);
            self.reload_history_pages();
        } else if let Some(value) = message.strip_prefix("history:visit-direction:") {
            self.history_visit_sort_desc = value != "asc";
            self.reload_history_pages();
        } else if let Some(value) = message.strip_prefix("history:open:") {
            let url = percent_decode(value);
            if !url.trim().is_empty() {
                let _ = self.create_tab(&url);
            }
        }
        self.save_state();
        self.refresh();
    }

    fn run_custom_keybind(&mut self, key: u32, ctrl: bool, alt: bool, shift: bool) -> bool {
        let combo = combo_label_for_event(key, ctrl, alt, shift);
        if combo.is_empty() {
            return false;
        }
        if let Some(action) = self
            .custom_keybinds
            .iter()
            .find(|(_, saved)| saved.eq_ignore_ascii_case(&combo))
            .map(|(action, _)| action.clone())
        {
            self.execute_keybind_action(&action);
            return true;
        }
        if let Some(default_action) = default_action_for_event(key, ctrl, alt, shift) {
            if self
                .custom_keybinds
                .iter()
                .any(|(action, _)| action == default_action)
            {
                return true;
            }
        }
        false
    }

    fn execute_keybind_action(&mut self, action: &str) {
        match action {
            "Navigate" => self.open_command(CommandMode::Navigate),
            "Bookmark site" => self.toggle_active_bookmark(),
            "Find in page" => self.open_find_bar(),
            "New tab" => self.open_command(CommandMode::NewTab),
            "Close tab" => {
                if let Some(index) = self.active_tab_index() {
                    self.close_tab(index);
                }
            }
            "Reload" => self.reload(),
            "Reset zoom" => self.reset_active_zoom(),
            "Zoom in" => self.adjust_active_zoom(0.1),
            "Zoom out" => self.adjust_active_zoom(-0.1),
            "Reopen closed tab" => self.reopen_closed_tab(),
            "Toggle sidebar" => self.toggle_sidebar(),
            "Go back" => self.go_back(),
            "Go forward" => self.go_forward(),
            "Switch tab above" => self.switch_tab_above(),
            "Switch tab below" => self.switch_tab_below(),
            "Split horizontal" => {
                if self.active_split_group_id().is_some() {
                    self.set_active_split_layout(SplitLayout::Horizontal);
                } else if let Some(index) = self.active_tab_index() {
                    self.split_tab_with_active(index, SplitLayout::Horizontal);
                }
            }
            "Split vertical" => {
                if self.active_split_group_id().is_some() {
                    self.set_active_split_layout(SplitLayout::Vertical);
                } else if let Some(index) = self.active_tab_index() {
                    self.split_tab_with_active(index, SplitLayout::Vertical);
                }
            }
            "Split grid" => {
                if self.active_split_group_id().is_some() {
                    self.set_active_split_layout(SplitLayout::Grid);
                } else if let Some(index) = self.active_tab_index() {
                    self.split_tab_with_active(index, SplitLayout::Grid);
                }
            }
            "Unsplit tabs" => {
                if let Some(group_id) = self.active_split_group_id() {
                    self.unsplit_group(group_id);
                }
            }
            "Toggle fullscreen" => self.toggle_fullscreen(),
            _ => {}
        }
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
        self.attach_web_message_handler(&webview);

        let id = self.next_id;
        self.next_id += 1;
        let index = self.tabs.len();
        let sidebar_order = self.allocate_sidebar_order();
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
            pinned_url: if pinned { Some(url.to_string()) } else { None },
            split_group_id: None,
            sidebar_order,
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
            is_sleeping: false,
            is_loading: false,
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
        if url == "aster:settings" {
            self.load_settings_page(index);
        } else if url == "aster:history" {
            self.load_history_page(index);
        } else if url == "aster:extensions" {
            self.load_extensions_page(index);
        } else {
            let wide = CoTaskMemPWSTR::from(url);
            unsafe {
                let _ = self.tabs[index]
                    .webview
                    .Navigate(*wide.as_ref().as_pcwstr());
            }
        }
        self.save_state();
        Ok(())
    }

    fn duplicate_tab(&mut self, index: usize, activate: bool) -> Option<usize> {
        let snapshot = self.tabs.get(index).map(|tab| {
            (
                tab.workspace_id,
                tab.folder_id,
                tab.pinned,
                tab.title.clone(),
                tab.pinned_url.clone().unwrap_or_else(|| tab.url.clone()),
                tab.history.clone(),
                tab.history_cursor,
            )
        })?;
        let (workspace_id, folder_id, pinned, title, url, history, history_cursor) = snapshot;
        if self
            .create_tab_in_workspace(&url, workspace_id, folder_id, pinned, activate, Some(title))
            .is_err()
        {
            return None;
        }
        let new_index = self.tabs.len().checked_sub(1)?;
        if let Some(tab) = self.tabs.get_mut(new_index) {
            tab.history = history;
            tab.history_cursor = history_cursor.min(tab.history.len().saturating_sub(1));
        }
        Some(new_index)
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

    fn tab_index_by_id(&self, tab_id: usize) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == tab_id)
    }

    fn split_group_index(&self, group_id: usize) -> Option<usize> {
        self.split_groups
            .iter()
            .position(|group| group.id == group_id)
    }

    fn split_group(&self, group_id: usize) -> Option<&SplitGroup> {
        self.split_groups.iter().find(|group| group.id == group_id)
    }

    fn split_group_active_index(&self, group_id: usize) -> Option<usize> {
        let group = self.split_group(group_id)?;
        self.tab_index_by_id(group.active_tab_id).or_else(|| {
            group
                .tab_ids
                .iter()
                .find_map(|tab_id| self.tab_index_by_id(*tab_id))
        })
    }

    fn split_group_for_tab_id(&self, tab_id: usize) -> Option<usize> {
        self.tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.split_group_id)
    }

    fn visible_tab_ids_for_index(&self, index: usize) -> Vec<usize> {
        let Some(tab) = self.tabs.get(index) else {
            return Vec::new();
        };
        if let Some(group_id) = tab.split_group_id {
            if let Some(group) = self.split_group(group_id) {
                return group
                    .tab_ids
                    .iter()
                    .copied()
                    .filter(|tab_id| {
                        self.tabs.iter().any(|candidate| {
                            candidate.id == *tab_id && candidate.workspace_id == tab.workspace_id
                        })
                    })
                    .collect();
            }
        }
        vec![tab.id]
    }

    #[allow(dead_code)]
    fn sanitize_split_groups(&mut self) {
        let mut surviving_groups = Vec::new();
        for mut group in self.split_groups.drain(..) {
            group.tab_ids.retain(|tab_id| {
                self.tabs
                    .iter()
                    .any(|tab| tab.id == *tab_id && tab.workspace_id == group.workspace_id)
            });
            if group.tab_ids.len() >= 2 {
                if !group.tab_ids.contains(&group.active_tab_id) {
                    group.active_tab_id = group.tab_ids[0];
                }
                surviving_groups.push(group);
            } else {
                for tab in &mut self.tabs {
                    if tab.split_group_id == Some(group.id) {
                        tab.split_group_id = None;
                    }
                }
            }
        }
        self.split_groups = surviving_groups;
    }

    fn detach_tab_from_split_by_id(&mut self, tab_id: usize) {
        let Some(group_id) = self.split_group_for_tab_id(tab_id) else {
            return;
        };
        if let Some(group_index) = self.split_group_index(group_id) {
            let remaining = {
                let group = &mut self.split_groups[group_index];
                group.tab_ids.retain(|id| *id != tab_id);
                if group.active_tab_id == tab_id {
                    if let Some(next) = group.tab_ids.first().copied() {
                        group.active_tab_id = next;
                    }
                }
                group.tab_ids.clone()
            };
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                tab.split_group_id = None;
            }
            if remaining.len() < 2 {
                for id in remaining {
                    if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                        tab.split_group_id = None;
                    }
                }
                self.split_groups.remove(group_index);
            }
        }
    }

    fn unsplit_group(&mut self, group_id: usize) {
        if let Some(index) = self.split_group_index(group_id) {
            let tab_ids = self.split_groups[index].tab_ids.clone();
            for tab_id in tab_ids {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.split_group_id = None;
                }
            }
            self.split_groups.remove(index);
            self.layout();
            self.save_state();
            self.refresh();
        }
    }

    fn active_split_group_id(&self) -> Option<usize> {
        self.active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.split_group_id)
    }

    fn set_active_split_layout(&mut self, layout: SplitLayout) {
        if let Some(group_id) = self.active_split_group_id() {
            if let Some(group) = self
                .split_groups
                .iter_mut()
                .find(|group| group.id == group_id)
            {
                group.layout = layout;
                self.layout();
                self.save_state();
                self.refresh();
            }
        }
    }

    fn cycle_active_split_layout(&mut self) {
        let next = self
            .active_split_group_id()
            .and_then(|group_id| self.split_group(group_id))
            .map(|group| match group.layout {
                SplitLayout::Horizontal => SplitLayout::Vertical,
                SplitLayout::Vertical => SplitLayout::Grid,
                SplitLayout::Grid => SplitLayout::Horizontal,
            });
        if let Some(layout) = next {
            self.set_active_split_layout(layout);
        }
    }

    fn split_tab_with_active(&mut self, index: usize, layout: SplitLayout) {
        let Some(active_index) = self.active_tab_index() else {
            return;
        };
        let target_index = if index == active_index {
            self.active_workspace_tabs()
                .into_iter()
                .find(|candidate| *candidate != active_index)
        } else {
            Some(index)
        };
        let Some(target_index) = target_index else {
            return;
        };
        let zone = match layout {
            SplitLayout::Horizontal => SplitDropZone::Right,
            SplitLayout::Vertical => SplitDropZone::Bottom,
            SplitLayout::Grid => SplitDropZone::Right,
        };
        let Some(target_tab_id) = self.tabs.get(active_index).map(|tab| tab.id) else {
            return;
        };
        let drop = SplitDropTarget {
            target_tab_id,
            zone,
        };
        self.apply_split_drop(target_index, drop, Some(layout));
    }

    fn apply_split_drop(
        &mut self,
        source_index: usize,
        target: SplitDropTarget,
        forced_layout: Option<SplitLayout>,
    ) {
        if source_index >= self.tabs.len() {
            return;
        }
        let source_tab_id = self.tabs[source_index].id;
        if source_tab_id == target.target_tab_id {
            return;
        }
        let Some(target_index) = self.tab_index_by_id(target.target_tab_id) else {
            return;
        };
        let target_workspace = self.tabs[target_index].workspace_id;
        let target_folder = self.tabs[target_index].folder_id;
        let target_pinned = self.tabs[target_index].pinned;
        let target_group_id = self.tabs[target_index].split_group_id;
        let source_group_id = self.split_group_for_tab_id(source_tab_id);

        if source_group_id.is_some() && source_group_id == target_group_id {
            let Some(group_id) = source_group_id else {
                return;
            };
            let Some(group_index) = self.split_group_index(group_id) else {
                return;
            };
            let group = &mut self.split_groups[group_index];
            if let Some(layout) = forced_layout {
                group.layout = layout;
            } else {
                group.layout = match target.zone {
                    SplitDropZone::Left | SplitDropZone::Right => SplitLayout::Horizontal,
                    SplitDropZone::Top | SplitDropZone::Bottom => SplitLayout::Vertical,
                };
            }
            group.tab_ids.retain(|id| *id != source_tab_id);
            let target_pos = group
                .tab_ids
                .iter()
                .position(|id| *id == target.target_tab_id)
                .unwrap_or(0);
            let insert_at = match target.zone {
                SplitDropZone::Left | SplitDropZone::Top => target_pos,
                SplitDropZone::Right | SplitDropZone::Bottom => target_pos + 1,
            }
            .min(group.tab_ids.len());
            group.tab_ids.insert(insert_at, source_tab_id);
            group.active_tab_id = source_tab_id;
            self.switch_to(source_index, true);
            self.save_state();
            self.refresh();
            return;
        }

        if target_group_id
            .and_then(|group_id| self.split_group(group_id))
            .map(|group| group.tab_ids.len() >= SPLIT_MAX_PANES)
            .unwrap_or(false)
        {
            return;
        }

        self.detach_tab_from_split_by_id(source_tab_id);

        let group_id = if let Some(group_id) = target_group_id {
            group_id
        } else {
            let group_id = self.next_split_group_id;
            self.next_split_group_id += 1;
            self.split_groups.push(SplitGroup {
                id: group_id,
                workspace_id: target_workspace,
                tab_ids: vec![target.target_tab_id],
                layout: forced_layout.unwrap_or(match target.zone {
                    SplitDropZone::Left | SplitDropZone::Right => SplitLayout::Horizontal,
                    SplitDropZone::Top | SplitDropZone::Bottom => SplitLayout::Vertical,
                }),
                active_tab_id: target.target_tab_id,
            });
            if let Some(tab) = self.tabs.get_mut(target_index) {
                tab.split_group_id = Some(group_id);
            }
            group_id
        };

        let Some(group_index) = self.split_group_index(group_id) else {
            return;
        };
        if self.split_groups[group_index].tab_ids.len() >= SPLIT_MAX_PANES {
            return;
        }

        let source_sidebar_order = self.allocate_sidebar_order();
        if let Some(tab) = self.tabs.get_mut(source_index) {
            tab.workspace_id = target_workspace;
            tab.folder_id = target_folder;
            tab.pinned = target_pinned;
            tab.pinned_url = if target_pinned {
                Some(tab.url.clone())
            } else {
                None
            };
            tab.split_group_id = Some(group_id);
            tab.sidebar_order = source_sidebar_order;
        }

        let group = &mut self.split_groups[group_index];
        group.workspace_id = target_workspace;
        if let Some(layout) = forced_layout {
            group.layout = layout;
        } else {
            group.layout = match target.zone {
                SplitDropZone::Left | SplitDropZone::Right => SplitLayout::Horizontal,
                SplitDropZone::Top | SplitDropZone::Bottom => SplitLayout::Vertical,
            };
        }
        group.tab_ids.retain(|id| *id != source_tab_id);
        let target_pos = group
            .tab_ids
            .iter()
            .position(|id| *id == target.target_tab_id)
            .unwrap_or(0);
        let insert_at = match target.zone {
            SplitDropZone::Left | SplitDropZone::Top => target_pos,
            SplitDropZone::Right | SplitDropZone::Bottom => target_pos + 1,
        }
        .min(group.tab_ids.len());
        group.tab_ids.insert(insert_at, source_tab_id);
        group.active_tab_id = source_tab_id;

        if let Some(source_index) = self.tab_index_by_id(source_tab_id) {
            self.active_workspace = target_workspace;
            self.switch_to(source_index, true);
        }
        self.save_state();
        self.refresh();
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
            .flat_map(|row| match row {
                SidebarRow::Tab(index) => vec![index],
                SidebarRow::SplitGroup(group_id) => self
                    .split_group(group_id)
                    .map(|group| {
                        group
                            .tab_ids
                            .iter()
                            .filter_map(|tab_id| self.tab_index_by_id(*tab_id))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                SidebarRow::TabGhost(_) | SidebarRow::Folder(_) | SidebarRow::Label(_) => {
                    Vec::new()
                }
            })
            .collect()
    }

    fn get_virtual_folder_state(&self, folder_id: usize) -> (Option<usize>, bool) {
        if let Some(drag) = self.drag_state {
            if drag.active {
                if let DragSource::Folder(from_id) = drag.source {
                    if folder_id == from_id {
                        let (parent_id, pinned) = match self.drop_target {
                            Some(DropTarget::PinnedSection) => (None, true),
                            Some(DropTarget::Folder(target_folder_id)) => {
                                if target_folder_id == from_id
                                    || self.is_descendant_of(target_folder_id, from_id)
                                {
                                    if let Some(f) = self.folders.iter().find(|f| f.id == folder_id)
                                    {
                                        (f.parent_id, f.pinned)
                                    } else {
                                        (None, false)
                                    }
                                } else {
                                    let target_pinned = self
                                        .folders
                                        .iter()
                                        .find(|f| f.id == target_folder_id)
                                        .map(|f| f.pinned)
                                        .unwrap_or(false);
                                    (Some(target_folder_id), target_pinned)
                                }
                            }
                            Some(DropTarget::Tab(target_tab_index)) => {
                                let is_tab_pinned = self
                                    .tabs
                                    .get(target_tab_index)
                                    .map(|t| t.pinned)
                                    .unwrap_or(false);
                                (None, is_tab_pinned)
                            }
                            Some(DropTarget::RootAfter { pinned, .. }) => (None, pinned),
                            Some(DropTarget::UnpinnedSection) | _ => (None, false),
                        };
                        return (parent_id, pinned);
                    }
                }
            }
        }
        if let Some(f) = self.folders.iter().find(|f| f.id == folder_id) {
            (f.parent_id, f.pinned)
        } else {
            (None, false)
        }
    }

    #[allow(dead_code)]
    fn get_virtual_tab_pinned(&self, tab_index: usize) -> bool {
        if let Some(tab) = self.tabs.get(tab_index) {
            if let Some(fid) = tab.folder_id {
                let (_, folder_pinned) = self.get_virtual_folder_state(fid);
                return folder_pinned;
            }
            return tab.pinned;
        }
        false
    }

    fn is_tab_in_folder_subtree(&self, tab_index: usize, folder_id: usize) -> bool {
        if let Some(tab) = self.tabs.get(tab_index) {
            if let Some(fid) = tab.folder_id {
                if fid == folder_id || self.is_descendant_of(fid, folder_id) {
                    return true;
                }
            }
        }
        false
    }

    fn is_preview_item(&self, row: SidebarRow) -> bool {
        if let Some(drag) = self.drag_state {
            if drag.active {
                match drag.source {
                    DragSource::Folder(from_folder_id) => match row {
                        SidebarRow::Folder(fid) => {
                            return fid == from_folder_id
                                || self.is_descendant_of(fid, from_folder_id);
                        }
                        SidebarRow::Tab(idx) => {
                            return self.is_tab_in_folder_subtree(idx, from_folder_id);
                        }
                        SidebarRow::TabGhost(_) => {}
                        _ => {}
                    },
                    DragSource::Tab(from_tab_index) => match row {
                        SidebarRow::Tab(idx) => {
                            if self.is_alt_duplicate_drag() {
                                return false;
                            }
                            return idx == from_tab_index;
                        }
                        SidebarRow::TabGhost(idx) => {
                            return idx == from_tab_index;
                        }
                        _ => {}
                    },
                }
            }
        }
        false
    }

    fn is_alt_duplicate_drag(&self) -> bool {
        self.drag_state
            .map(|drag| drag.active && matches!(drag.source, DragSource::Tab(_)))
            .unwrap_or(false)
            && unsafe { (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 }
    }

    fn folder_depth(&self, folder_id: usize) -> usize {
        let mut depth = 0;
        let mut current_id = folder_id;
        let mut visited = std::collections::HashSet::new();
        while let Some(_folder) = self.folders.iter().find(|f| f.id == current_id) {
            if !visited.insert(current_id) {
                break;
            }
            let (parent_id, _) = self.get_virtual_folder_state(current_id);
            if let Some(parent) = parent_id {
                depth += 1;
                current_id = parent;
            } else {
                break;
            }
        }
        depth
    }

    fn tab_depth(&self, index: usize) -> usize {
        if let Some(drag) = self.drag_state {
            if drag.active {
                if let DragSource::Tab(from_index) = drag.source {
                    if from_index == index {
                        match self.drop_target {
                            Some(DropTarget::PinnedSection) => return 0,
                            Some(DropTarget::Folder(folder_id)) => {
                                return self.folder_depth(folder_id) + 1;
                            }
                            Some(DropTarget::Tab(target_tab_index)) => {
                                if target_tab_index != index {
                                    return self.tab_depth(target_tab_index);
                                }
                            }
                            Some(DropTarget::RootAfter { .. }) => return 0,
                            Some(DropTarget::UnpinnedSection) | _ => return 0,
                        }
                    }
                }
            }
        }
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
                tab.pinned_url = if pinned { Some(tab.url.clone()) } else { None };
            }
        }
        let child_folder_ids: Vec<usize> = self
            .folders
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

    fn row_sidebar_order(&self, row: SidebarRow) -> u64 {
        match row {
            SidebarRow::Folder(folder_id) => self
                .folders
                .iter()
                .find(|folder| folder.id == folder_id)
                .map(|folder| folder.sidebar_order)
                .unwrap_or(u64::MAX),
            SidebarRow::SplitGroup(group_id) => self
                .split_group(group_id)
                .map(|group| {
                    group
                        .tab_ids
                        .iter()
                        .filter_map(|tab_id| {
                            self.tabs
                                .iter()
                                .find(|tab| tab.id == *tab_id)
                                .map(|tab| tab.sidebar_order)
                        })
                        .min()
                        .unwrap_or(u64::MAX)
                })
                .unwrap_or(u64::MAX),
            SidebarRow::Tab(tab_index) => self
                .tabs
                .get(tab_index)
                .map(|tab| tab.sidebar_order)
                .unwrap_or(u64::MAX),
            SidebarRow::TabGhost(tab_index) => self
                .tabs
                .get(tab_index)
                .map(|tab| tab.sidebar_order)
                .unwrap_or(u64::MAX),
            SidebarRow::Label(_) => 0,
        }
    }

    fn sorted_sidebar_rows(&self, mut rows: Vec<SidebarRow>) -> Vec<SidebarRow> {
        rows.sort_by_key(|row| self.row_sidebar_order(*row));
        rows
    }

    fn coalesce_split_rows(&self, rows: Vec<SidebarRow>) -> Vec<SidebarRow> {
        let mut seen_groups = Vec::new();
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            match row {
                SidebarRow::Tab(index) => {
                    let group_id = self.tabs.get(index).and_then(|tab| tab.split_group_id);
                    if let Some(group_id) = group_id {
                        if !seen_groups.contains(&group_id) {
                            seen_groups.push(group_id);
                            out.push(SidebarRow::SplitGroup(group_id));
                        }
                    } else {
                        out.push(row);
                    }
                }
                SidebarRow::TabGhost(_) => out.push(row),
                SidebarRow::Folder(_) | SidebarRow::Label(_) | SidebarRow::SplitGroup(_) => {
                    out.push(row)
                }
            }
        }
        out
    }

    fn root_section_rows(&self, pinned: bool) -> Vec<SidebarRow> {
        let folders = self
            .folders
            .iter()
            .filter(|folder| {
                folder.workspace_id == self.active_workspace
                    && folder.pinned == pinned
                    && folder.parent_id.is_none()
            })
            .map(|folder| SidebarRow::Folder(folder.id));
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace
                    && tab.pinned == pinned
                    && tab.folder_id.is_none()
            })
            .map(|(index, _)| SidebarRow::Tab(index));
        self.coalesce_split_rows(self.sorted_sidebar_rows(folders.chain(tabs).collect()))
    }

    fn assign_row_sidebar_order(&mut self, row: SidebarRow, sidebar_order: u64) {
        match row {
            SidebarRow::Folder(folder_id) => {
                if let Some(folder) = self
                    .folders
                    .iter_mut()
                    .find(|folder| folder.id == folder_id)
                {
                    folder.sidebar_order = sidebar_order;
                }
            }
            SidebarRow::Tab(tab_index) => {
                if let Some(tab) = self.tabs.get_mut(tab_index) {
                    tab.sidebar_order = sidebar_order;
                }
            }
            SidebarRow::SplitGroup(group_id) => {
                let tab_ids = self
                    .split_group(group_id)
                    .map(|group| group.tab_ids.clone())
                    .unwrap_or_default();
                for (offset, tab_id) in tab_ids.into_iter().enumerate() {
                    if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                        tab.sidebar_order = sidebar_order + offset as u64;
                    }
                }
            }
            SidebarRow::TabGhost(_) => {}
            SidebarRow::Label(_) => {}
        }
    }

    fn assign_root_section_orders(&mut self, rows: Vec<SidebarRow>) {
        for (index, row) in rows.into_iter().enumerate() {
            self.assign_row_sidebar_order(row, ((index as u64) + 1) * 1024);
        }
    }

    fn place_root_row_after(
        &mut self,
        moved: SidebarRow,
        pinned: bool,
        target: Option<SidebarRow>,
    ) {
        let mut rows: Vec<SidebarRow> = self
            .root_section_rows(pinned)
            .into_iter()
            .filter(|row| *row != moved)
            .collect();
        let insert_at = target
            .and_then(|target| {
                rows.iter()
                    .position(|row| *row == target)
                    .map(|pos| pos + 1)
            })
            .unwrap_or(rows.len());
        rows.insert(insert_at.min(rows.len()), moved);
        self.assign_root_section_orders(rows);
    }

    fn place_root_row_at_start(&mut self, moved: SidebarRow, pinned: bool) {
        let mut rows: Vec<SidebarRow> = self
            .root_section_rows(pinned)
            .into_iter()
            .filter(|row| *row != moved)
            .collect();
        rows.insert(0, moved);
        self.assign_root_section_orders(rows);
    }

    fn add_folder_rows_recursive(&self, folder_id: usize, rows: &mut Vec<SidebarRow>) {
        let child_folders = self
            .folders
            .iter()
            .filter(|f| f.workspace_id == self.active_workspace && f.parent_id == Some(folder_id))
            .map(|folder| SidebarRow::Folder(folder.id));
        let child_tabs: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace && tab.folder_id == Some(folder_id)
            })
            .map(|(index, _)| index)
            .collect();
        let child_tabs = child_tabs.into_iter().map(SidebarRow::Tab);
        for row in self.coalesce_split_rows(self.sorted_sidebar_rows(
            child_folders.chain(child_tabs).collect(),
        )) {
            rows.push(row);
            if let SidebarRow::Folder(child_folder_id) = row {
                if let Some(folder) = self.folders.iter().find(|f| f.id == child_folder_id) {
                    if !folder.collapsed {
                        self.add_folder_rows_recursive(child_folder_id, rows);
                    }
                }
            }
        }
    }

    fn preview_insert_index(
        &self,
        rows: &[SidebarRow],
        after: Option<SidebarRow>,
    ) -> Option<usize> {
        match after {
            Some(target) => rows
                .iter()
                .position(|row| *row == target)
                .map(|pos| pos + 1),
            None => Some(rows.len()),
        }
    }

    fn sidebar_rows(&self) -> Vec<SidebarRow> {
        let mut rows = Vec::new();

        // Pinned Section
        let mut pinned_rows = Vec::new();
        let root_pinned_folders = self
            .folders
            .iter()
            .filter(|f| {
                f.workspace_id == self.active_workspace && f.pinned && f.parent_id.is_none()
            })
            .map(|folder| SidebarRow::Folder(folder.id));
        let loose_pinned_tabs: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace && tab.pinned && tab.folder_id.is_none()
            })
            .map(|(index, _)| index)
            .collect();
        let loose_pinned_tabs = loose_pinned_tabs.into_iter().map(SidebarRow::Tab);
        for row in self.coalesce_split_rows(self.sorted_sidebar_rows(
            root_pinned_folders.chain(loose_pinned_tabs).collect(),
        )) {
            pinned_rows.push(row);
            if let SidebarRow::Folder(folder_id) = row {
                if let Some(folder) = self.folders.iter().find(|f| f.id == folder_id) {
                    if !folder.collapsed {
                        self.add_folder_rows_recursive(folder_id, &mut pinned_rows);
                    }
                }
            }
        }
        rows.extend(pinned_rows);

        // Always push the divider line!
        rows.push(SidebarRow::Label(SidebarLabel::Tabs));

        // Unpinned Section
        let mut unpinned_rows = Vec::new();
        let root_unpinned_folders = self
            .folders
            .iter()
            .filter(|f| {
                f.workspace_id == self.active_workspace && !f.pinned && f.parent_id.is_none()
            })
            .map(|folder| SidebarRow::Folder(folder.id));
        let loose_tabs: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.workspace_id == self.active_workspace && !tab.pinned && tab.folder_id.is_none()
            })
            .map(|(index, _)| index)
            .collect();
        let loose_tabs = loose_tabs.into_iter().map(SidebarRow::Tab);
        for row in self.coalesce_split_rows(self.sorted_sidebar_rows(
            root_unpinned_folders.chain(loose_tabs).collect(),
        )) {
            unpinned_rows.push(row);
            if let SidebarRow::Folder(folder_id) = row {
                if let Some(folder) = self.folders.iter().find(|f| f.id == folder_id) {
                    if !folder.collapsed {
                        self.add_folder_rows_recursive(folder_id, &mut unpinned_rows);
                    }
                }
            }
        }
        rows.extend(unpinned_rows);

        // If dragging a folder or a tab, modify rows list to show ghost preview at target position
        if let Some(drag) = self.drag_state {
            if drag.active {
                match drag.source {
                    DragSource::Folder(from_folder_id) => {
                        // 1. Filter out the dragged folder and its descendants
                        let subtree_rows = vec![SidebarRow::Folder(from_folder_id)];
                        let mut base_rows: Vec<SidebarRow> = rows
                            .into_iter()
                            .filter(|row| match *row {
                                SidebarRow::Folder(fid) => {
                                    fid != from_folder_id
                                        && !self.is_descendant_of(fid, from_folder_id)
                                }
                                SidebarRow::Tab(idx) => {
                                    !self.is_tab_in_folder_subtree(idx, from_folder_id)
                                }
                                SidebarRow::SplitGroup(group_id) => self
                                    .split_group(group_id)
                                    .map(|group| {
                                        !group.tab_ids.iter().any(|tab_id| {
                                            self.tab_index_by_id(*tab_id)
                                                .map(|idx| {
                                                    self.is_tab_in_folder_subtree(
                                                        idx,
                                                        from_folder_id,
                                                    )
                                                })
                                                .unwrap_or(false)
                                        })
                                    })
                                    .unwrap_or(true),
                                SidebarRow::TabGhost(_) => false,
                                SidebarRow::Label(_) => true,
                            })
                            .collect();

                        // 2. Find insertion index
                        let insert_index = match self.drop_target {
                            Some(DropTarget::PinnedSection) => {
                                // Top of pinned section (index 0)
                                Some(0)
                            }
                            Some(DropTarget::UnpinnedSection) => Some(base_rows.len()),
                            Some(DropTarget::RootAfter { row, .. }) => {
                                self.preview_insert_index(&base_rows, row)
                            }
                            Some(DropTarget::Folder(target_folder_id)) => {
                                // Inside target folder: put it right after the folder header row
                                base_rows.iter().position(|r| matches!(r, SidebarRow::Folder(fid) if *fid == target_folder_id))
                                    .map(|pos| pos + 1)
                                    .or(Some(0))
                            }
                            Some(DropTarget::Tab(target_tab_index)) => {
                                // Insert after the last root folder of the same pinned type —
                                // this matches handle_drop, which always places root folders
                                // in the folders-first, tabs-second section layout.
                                base_rows.iter().position(|r| matches!(r, SidebarRow::Tab(idx) if *idx == target_tab_index))
                                    .map(|pos| pos + 1)
                                    .or(Some(0))
                            }
                            Some(DropTarget::None) | None => None,
                        };

                        // 3. Insert the subtree at the computed index
                        if let Some(insert_index) = insert_index {
                            let insert_index = insert_index.min(base_rows.len());
                            for (i, item) in subtree_rows.into_iter().enumerate() {
                                base_rows.insert(insert_index + i, item);
                            }
                        }
                        rows = base_rows;
                    }
                    DragSource::Tab(from_tab_index) => {
                        let duplicate_drag = self.is_alt_duplicate_drag();
                        let mut base_rows: Vec<SidebarRow> = rows
                            .into_iter()
                            .filter(|row| match *row {
                                SidebarRow::Tab(idx) => duplicate_drag || idx != from_tab_index,
                                SidebarRow::SplitGroup(group_id) => {
                                    let from_tab_id =
                                        self.tabs.get(from_tab_index).map(|tab| tab.id);
                                    duplicate_drag
                                        || self
                                            .split_group(group_id)
                                            .zip(from_tab_id)
                                            .map(|(group, tab_id)| {
                                                !group.tab_ids.contains(&tab_id)
                                            })
                                            .unwrap_or(true)
                                }
                                SidebarRow::TabGhost(_) => false,
                                _ => true,
                            })
                            .collect();

                        // 2. Find insertion index
                        let insert_index = match self.drop_target {
                            Some(DropTarget::PinnedSection) => {
                                // Top of pinned section (index 0)
                                Some(0)
                            }
                            Some(DropTarget::UnpinnedSection) => Some(base_rows.len()),
                            Some(DropTarget::RootAfter { row, .. }) => {
                                self.preview_insert_index(&base_rows, row)
                            }
                            Some(DropTarget::Folder(target_folder_id)) => {
                                // Inside target folder: put it right after the folder header row
                                base_rows.iter().position(|r| matches!(r, SidebarRow::Folder(fid) if *fid == target_folder_id))
                                    .map(|pos| pos + 1)
                                    .or(Some(0))
                            }
                            Some(DropTarget::Tab(target_tab_index)) => {
                                // After the hovered tab
                                base_rows.iter().position(|r| matches!(r, SidebarRow::Tab(idx) if *idx == target_tab_index))
                                    .map(|pos| pos + 1)
                                    .or(Some(0))
                            }
                            Some(DropTarget::None) | None => None,
                        };

                        // 3. Insert the tab at the computed index
                        if let Some(insert_index) = insert_index {
                            let insert_index = insert_index.min(base_rows.len());
                            let row = if duplicate_drag {
                                SidebarRow::TabGhost(from_tab_index)
                            } else {
                                SidebarRow::Tab(from_tab_index)
                            };
                            base_rows.insert(insert_index, row);
                        }
                        rows = base_rows;
                    }
                }
            }
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
        let has_pinned = self
            .folders
            .iter()
            .any(|f| f.workspace_id == self.active_workspace && f.pinned)
            || self
                .tabs
                .iter()
                .any(|t| t.workspace_id == self.active_workspace && t.pinned);
        let mut y = if has_pinned {
            self.sidebar_rows_top()
        } else {
            self.sidebar_rows_top() + 72
        };
        let all_rows = self.sidebar_rows();
        let effective_offset = self
            .sidebar_scroll_offset
            .min(all_rows.len().saturating_sub(1));
        for skipped in all_rows.iter().take(effective_offset) {
            y += match skipped {
                SidebarRow::Label(_) => 24,
                SidebarRow::Folder(_) => 36,
                SidebarRow::SplitGroup(_) => 50,
                SidebarRow::Tab(_) | SidebarRow::TabGhost(_) => 44,
            };
        }
        for row in all_rows.into_iter().skip(effective_offset) {
            let height = match row {
                SidebarRow::Label(_) => 24,
                SidebarRow::Folder(_) => 36,
                SidebarRow::SplitGroup(_) => 50,
                SidebarRow::Tab(_) | SidebarRow::TabGhost(_) => 44,
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

    fn sidebar_row_contains_tab(&self, row: SidebarRow, index: usize) -> bool {
        match row {
            SidebarRow::Tab(row_index) => row_index == index,
            SidebarRow::SplitGroup(group_id) => {
                let Some(tab_id) = self.tabs.get(index).map(|tab| tab.id) else {
                    return false;
                };
                self.split_group(group_id)
                    .map(|group| group.tab_ids.contains(&tab_id))
                    .unwrap_or(false)
            }
            SidebarRow::Label(_) | SidebarRow::Folder(_) | SidebarRow::TabGhost(_) => false,
        }
    }

    fn sidebar_row_rect_for_tab(&self, index: usize) -> Option<RECT> {
        self.sidebar_row_rects()
            .into_iter()
            .find_map(|(row, rect)| {
                if self.sidebar_row_contains_tab(row, index) {
                    Some(rect)
                } else {
                    None
                }
            })
    }

    fn topbar_pushed_height(&self) -> i32 {
        if self.topbar_mode == SidebarMode::Pushed || self.topbar_expand_mode == SidebarMode::Pushed
        {
            self.topbar_height as i32
        } else {
            0
        }
    }

    fn topbar_y(&self) -> i32 {
        (self.topbar_height as i32) - TOPBAR_HEIGHT
    }

    fn sidebar_header_top(&self) -> i32 {
        self.topbar_pushed_height() + TOPBAR_HEIGHT - 4
    }

    fn sidebar_rows_top(&self) -> i32 {
        self.topbar_pushed_height() + TOPBAR_HEIGHT + 50
    }

    fn workspace_header_rect(&self) -> RECT {
        RECT {
            left: 12,
            top: self.sidebar_header_top(),
            right: self.sidebar_width() - 12,
            bottom: self.sidebar_header_top() + 38,
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
        let has_pinned = self
            .folders
            .iter()
            .any(|f| f.workspace_id == self.active_workspace && f.pinned)
            || self
                .tabs
                .iter()
                .any(|t| t.workspace_id == self.active_workspace && t.pinned);
        if !has_pinned {
            let y = self.sidebar_rows_top();
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
                    SidebarRow::SplitGroup(group_id) => {
                        self.split_group_active_index(group_id).map(SidebarHit::Tab)
                    }
                    SidebarRow::Tab(index) => Some(SidebarHit::Tab(index)),
                    SidebarRow::TabGhost(_) | SidebarRow::Label(_) => None,
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
        self.bookmark_folders.clear();
        self.bookmarks.clear();
        self.tabs.clear();
        self.split_groups.clear();
        self.workspace_active_tabs.clear();
        self.visited_sites.clear();
        self.custom_keybinds.clear();

        let mut tab_records = Vec::new();
        let mut active_workspace = 1usize;
        for (line_index, line) in raw.lines().enumerate() {
            let fallback_sidebar_order = ((line_index as u64) + 1) * 1024;
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
                        let parent_id = parts.get(6).and_then(|val| {
                            if val.is_empty() {
                                None
                            } else {
                                val.parse::<usize>().ok()
                            }
                        });
                        self.folders.push(Folder {
                            id,
                            workspace_id,
                            parent_id,
                            name: parts[3].clone(),
                            collapsed: parts.get(4).map(|value| value == "1").unwrap_or(false),
                            pinned: parts.get(5).map(|value| value == "1").unwrap_or(false),
                            sidebar_order: parts
                                .get(7)
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or(fallback_sidebar_order),
                        });
                    }
                }
                "bookmark_folder" if parts.len() >= 4 => {
                    if let Ok(id) = parts[1].parse::<usize>() {
                        let parent_id = if parts[2].is_empty() {
                            None
                        } else {
                            parts[2].parse::<usize>().ok()
                        };
                        self.bookmark_folders.push(BookmarkFolder {
                            id,
                            parent_id,
                            name: parts[3].clone(),
                            sidebar_order: parts
                                .get(4)
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or(fallback_sidebar_order),
                        });
                    }
                }
                "bookmark" if parts.len() >= 5 => {
                    if let Ok(id) = parts[1].parse::<usize>() {
                        let folder_id = if parts[2].is_empty() {
                            None
                        } else {
                            parts[2].parse::<usize>().ok()
                        };
                        let tags = parts
                            .get(5)
                            .map(|raw| parse_tag_list(raw))
                            .unwrap_or_default();
                        self.bookmarks.push(Bookmark {
                            id,
                            folder_id,
                            title: parts[3].clone(),
                            url: parts[4].clone(),
                            tags,
                            created_at: parts
                                .get(6)
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or_else(current_timestamp),
                            sidebar_order: parts
                                .get(7)
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or(fallback_sidebar_order),
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
                            parts
                                .get(7)
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or(fallback_sidebar_order),
                        ));
                    }
                }
                "suggestion" if parts.len() >= 2 => {
                    let url = parts[1].clone();
                    let visit_count = parts
                        .get(2)
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(1);
                    let last_visit_time = parts
                        .get(3)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or_else(|| current_timestamp());
                    let title = parts
                        .get(4)
                        .filter(|title| !title.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| label_for_url(&url));
                    self.visited_sites.push(VisitedSite {
                        title,
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
                "setting" if parts.len() >= 3 => match parts[1].as_str() {
                    "dominant_color" => {
                        if let Ok(color) = parts[2].parse::<u32>() {
                            self.dominant_color = color;
                        }
                    }
                    "secondary_color" => {
                        if let Ok(color) = parts[2].parse::<u32>() {
                            self.secondary_color = color;
                            self.recreate_secondary_brush();
                        }
                    }
                    "accent_color" => {
                        if let Ok(color) = parts[2].parse::<u32>() {
                            self.accent_color = color;
                        }
                    }
                    "site_mode" => {
                        self.site_mode = match parts[2].as_str() {
                            "dark" => SiteMode::Dark,
                            "light" => SiteMode::Light,
                            _ => SiteMode::Auto,
                        };
                    }
                    "history_sort_mode" => {
                        self.history_sort_mode = HistorySortMode::from_str(&parts[2]);
                    }
                    "history_visit_sort_desc" => {
                        self.history_visit_sort_desc = parts[2] != "asc";
                    }
                    "startup_mode" => {
                        self.startup_mode = match parts[2].as_str() {
                            "home" => StartupMode::HomePage,
                            _ => StartupMode::LastSession,
                        };
                    }
                    "default_bubble_dismissed" => {
                        self.default_bubble_dismissed = parts[2] == "1";
                    }
                    _ => {}
                },
                "keybind" if parts.len() >= 3 => {
                    self.custom_keybinds
                        .push((parts[1].clone(), parts[2].clone()));
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
        self.ensure_default_bookmark_folder();
        self.next_bookmark_folder_id = self
            .bookmark_folders
            .iter()
            .map(|folder| folder.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.next_bookmark_id = self
            .bookmarks
            .iter()
            .map(|bookmark| bookmark.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.next_sidebar_order = self
            .folders
            .iter()
            .map(|folder| folder.sidebar_order)
            .max()
            .unwrap_or(0)
            .saturating_add(1024);
        self.active_workspace = if self
            .workspaces
            .iter()
            .any(|workspace| workspace.id == active_workspace)
        {
            active_workspace
        } else {
            self.workspaces[0].id
        };

        for (workspace_id, folder_id, pinned, title, url, history, sidebar_order) in tab_records {
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
                    tab.sidebar_order = sidebar_order;
                    tab.unloaded = true;
                    if !history.is_empty() {
                        tab.history = history;
                        tab.history_cursor = tab.history.len().saturating_sub(1);
                    }
                }
            }
        }
        self.next_sidebar_order = self
            .folders
            .iter()
            .map(|folder| folder.sidebar_order)
            .chain(self.tabs.iter().map(|tab| tab.sidebar_order))
            .max()
            .unwrap_or(0)
            .saturating_add(1024);
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
        lines.push(format!("setting\tdominant_color\t{}", self.dominant_color));
        lines.push(format!(
            "setting\tsecondary_color\t{}",
            self.secondary_color
        ));
        lines.push(format!("setting\taccent_color\t{}", self.accent_color));
        lines.push(format!(
            "setting\tsite_mode\t{}",
            match self.site_mode {
                SiteMode::Auto => "auto",
                SiteMode::Dark => "dark",
                SiteMode::Light => "light",
            }
        ));
        lines.push(format!(
            "setting\thistory_sort_mode\t{}",
            self.history_sort_mode.as_str()
        ));
        lines.push(format!(
            "setting\thistory_visit_sort_desc\t{}",
            if self.history_visit_sort_desc {
                "desc"
            } else {
                "asc"
            }
        ));
        lines.push(format!(
            "setting\tstartup_mode\t{}",
            match self.startup_mode {
                StartupMode::HomePage => "home",
                StartupMode::LastSession => "last",
            }
        ));
        lines.push(format!(
            "setting\tdefault_bubble_dismissed\t{}",
            if self.default_bubble_dismissed {
                "1"
            } else {
                "0"
            }
        ));
        for (action, combo) in &self.custom_keybinds {
            lines.push(format!(
                "keybind\t{}\t{}",
                escape_state(action),
                escape_state(combo)
            ));
        }
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
                "folder\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                folder.id,
                folder.workspace_id,
                escape_state(&folder.name),
                if folder.collapsed { "1" } else { "0" },
                if folder.pinned { "1" } else { "0" },
                folder
                    .parent_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                folder.sidebar_order
            ));
        }
        for folder in &self.bookmark_folders {
            lines.push(format!(
                "bookmark_folder\t{}\t{}\t{}\t{}",
                folder.id,
                folder
                    .parent_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                escape_state(&folder.name),
                folder.sidebar_order
            ));
        }
        for bookmark in &self.bookmarks {
            lines.push(format!(
                "bookmark\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                bookmark.id,
                bookmark
                    .folder_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                escape_state(&bookmark.title),
                escape_state(&bookmark.url),
                serialize_tag_list(&bookmark.tags),
                bookmark.created_at,
                bookmark.sidebar_order
            ));
        }
        for tab in &self.tabs {
            if tab.url.trim().is_empty() {
                continue;
            }
            let url_to_save = if tab.pinned {
                tab.pinned_url.as_ref().unwrap_or(&tab.url)
            } else {
                &tab.url
            };
            lines.push(format!(
                "tab\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                tab.workspace_id,
                tab.folder_id.map(|id| id.to_string()).unwrap_or_default(),
                if tab.pinned { "1" } else { "0" },
                escape_state(&tab.title),
                escape_state(url_to_save),
                serialize_history(&tab.history),
                tab.sidebar_order
            ));
        }
        for site in self.visited_sites.iter().take(500) {
            lines.push(format!(
                "suggestion\t{}\t{}\t{}\t{}",
                escape_state(&site.url),
                site.visit_count,
                site.last_visit_time,
                escape_state(&site.title)
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
            webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_sender, _args| {
                    with_app(hwnd, |app| app.set_tab_loading(tab_id, true));
                    Ok(())
                })),
                &mut token,
            )?;

            let hwnd = self.hwnd;
            let mut token = 0;
            webview.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_sender, _args| {
                    with_app(hwnd, |app| {
                        app.set_tab_loading(tab_id, false);
                        if app.find_open {
                            app.run_find_script(0);
                        }
                    });
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

            if let Ok(webview4) = webview.cast::<ICoreWebView2_4>() {
                let hwnd = self.hwnd;
                let mut token = 0;
                webview4.add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(move |_sender, args| {
                        if let Some(args) = args {
                            let _ = args.SetHandled(true);
                            if let Ok(operation) = args.DownloadOperation() {
                                let mut result_path = PWSTR::null();
                                let file_path = if args.ResultFilePath(&mut result_path).is_ok() {
                                    CoTaskMemPWSTR::from(result_path).to_string()
                                } else {
                                    String::new()
                                };
                                with_app(hwnd, |app| {
                                    let id = app.register_download(operation.clone(), file_path);
                                    app.attach_download_events(id, &operation);
                                });
                            }
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
            }

            let hwnd = self.hwnd;
            let mut token = 0;
            webview.add_ContainsFullScreenElementChanged(
                &ContainsFullScreenElementChangedEventHandler::create(Box::new(
                    move |sender, _args| {
                        if let Some(sender) = sender {
                            let mut contains = BOOL::from(false);
                            if sender.ContainsFullScreenElement(&mut contains).is_ok() {
                                with_app(hwnd, |app| {
                                    app.set_fullscreen_state(contains.as_bool());
                                });
                            }
                        }
                        Ok(())
                    },
                )),
                &mut token,
            )?;
        }

        if index_hint == usize::MAX {
            unreachable!();
        }
        Ok(())
    }

    fn register_download(
        &mut self,
        operation: ICoreWebView2DownloadOperation,
        suggested_path: String,
    ) -> usize {
        let id = self.next_download_id;
        self.next_download_id += 1;
        let snapshot = download_snapshot(&operation);
        let file_path = if suggested_path.is_empty() {
            snapshot.file_path
        } else {
            suggested_path
        };
        let file_name = download_file_name(&file_path, &snapshot.uri);
        let old_count = self.downloads.len();
        self.downloads.push(DownloadItem {
            id,
            file_name,
            file_path,
            uri: snapshot.uri,
            received_bytes: snapshot.received_bytes,
            total_bytes: snapshot.total_bytes,
            state: snapshot.state,
            paused: false,
            completed_at: None,
            cancelled_at: None,
            operation: Some(operation),
        });
        if self.downloads.len() == 4 && old_count == 3 {
            self.download_collapse_anim = Some(DownloadCollapseAnim {
                start_time: std::time::Instant::now(),
                duration: 180,
            });
        }
        if self.sidebar_width() <= 92 {
            self.download_toast = Some(DownloadToastState {
                start_time: std::time::Instant::now(),
                fading: false,
                slide_x: 0.0,
            });
            if self.sidebar_width() < 1 && self.download_popup_hwnd != HWND(std::ptr::null_mut()) {
                let rect = client_rect(self.hwnd);
                unsafe {
                    let _ = WindowsAndMessaging::SetWindowPos(
                        self.download_popup_hwnd,
                        Some(HWND_TOP),
                        116,
                        rect.bottom - 52,
                        32,
                        32,
                        WindowsAndMessaging::SWP_NOACTIVATE | WindowsAndMessaging::SWP_SHOWWINDOW,
                    );
                }
            }
        }
        self.ensure_download_timer();
        self.refresh();
        id
    }

    fn attach_download_events(
        &self,
        download_id: usize,
        operation: &ICoreWebView2DownloadOperation,
    ) {
        unsafe {
            let hwnd = self.hwnd;
            let mut token = 0;
            let _ = operation.add_BytesReceivedChanged(
                &BytesReceivedChangedEventHandler::create(Box::new(move |sender, _args| {
                    if let Some(sender) = sender {
                        with_app(hwnd, |app| {
                            app.update_download_from_operation(download_id, &sender)
                        });
                    }
                    Ok(())
                })),
                &mut token,
            );

            let hwnd = self.hwnd;
            let mut token = 0;
            let _ = operation.add_StateChanged(
                &StateChangedEventHandler::create(Box::new(move |sender, _args| {
                    if let Some(sender) = sender {
                        with_app(hwnd, |app| {
                            app.update_download_from_operation(download_id, &sender)
                        });
                    }
                    Ok(())
                })),
                &mut token,
            );
        }
    }

    fn update_download_from_operation(
        &mut self,
        download_id: usize,
        operation: &ICoreWebView2DownloadOperation,
    ) {
        let snapshot = download_snapshot(operation);
        if let Some(download) = self
            .downloads
            .iter_mut()
            .find(|item| item.id == download_id)
        {
            // Only update byte counts if we got valid data from WebView2
            // (total_bytes > 0 indicates the COM calls succeeded)
            if snapshot.total_bytes > 0 {
                download.received_bytes = snapshot.received_bytes;
                download.total_bytes = snapshot.total_bytes;
            }
            if !snapshot.file_path.is_empty() {
                download.file_path = snapshot.file_path;
                download.file_name = download_file_name(&download.file_path, &download.uri);
            }
            if !snapshot.uri.is_empty() {
                download.uri = snapshot.uri;
                if download.file_name == "download" {
                    download.file_name = download_file_name(&download.file_path, &download.uri);
                }
            }
            if download.state != COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED
                && snapshot.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED
            {
                download.completed_at = Some(std::time::Instant::now());
                download.paused = false;
            }
            if snapshot.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
                download.paused = false;
                if download.state != COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
                    download.cancelled_at = Some(std::time::Instant::now());
                }
            }
            download.state = snapshot.state;
        }
        self.ensure_download_timer();
        self.refresh();
    }

    fn tick_download_toast(&mut self) {
        if let Some(toast) = &mut self.download_toast {
            if toast.fading {
                if self.sidebar_width >= SIDEBAR_EXPANDED {
                    if self.download_popup_hwnd != HWND(std::ptr::null_mut()) {
                        unsafe {
                            let _ = WindowsAndMessaging::ShowWindow(
                                self.download_popup_hwnd,
                                WindowsAndMessaging::SW_HIDE,
                            );
                        }
                    }
                    self.download_toast = None;
                }
            } else {
                let elapsed = toast.start_time.elapsed().as_millis();
                if elapsed >= 200 {
                    let slide_elapsed = elapsed - 200;
                    let slide_duration: u128 = 400;
                    if slide_elapsed >= slide_duration {
                        if self.download_popup_hwnd != HWND(std::ptr::null_mut()) {
                            unsafe {
                                let _ = WindowsAndMessaging::ShowWindow(
                                    self.download_popup_hwnd,
                                    WindowsAndMessaging::SW_HIDE,
                                );
                            }
                        }
                        self.download_toast = None;
                    } else {
                        let t = slide_elapsed as f32 / slide_duration as f32;
                        let ease = 1.0 - (1.0 - t) * (1.0 - t);
                        toast.slide_x = -148.0 * ease;
                        let rect = client_rect(self.hwnd);
                        unsafe {
                            let _ = WindowsAndMessaging::SetWindowPos(
                                self.download_popup_hwnd,
                                Some(HWND_TOP),
                                (116.0 + toast.slide_x) as i32,
                                rect.bottom - 52,
                                32,
                                32,
                                WindowsAndMessaging::SWP_NOACTIVATE,
                            );
                        }
                    }
                }
            }
        }
    }

    fn tick_download_panel_animation(&mut self) {
        if self.download_panel.is_none() {
            return;
        }
        let distance = self.download_panel_reveal_target - self.download_panel_reveal;
        if distance.abs() < 0.005 {
            self.download_panel_reveal = self.download_panel_reveal_target;
            if self.download_panel_reveal < 0.01 {
                self.download_panel = None;
                self.download_panel_reveal = 0.0;
            }
        } else {
            self.download_panel_reveal += distance * 0.35;
        }
    }

    fn tick_download_removal(&mut self) {
        if let Some(anim) = &self.download_removal_anim {
            if anim.start_time.elapsed().as_millis() >= anim.duration as u128 {
                self.download_removal_anim = None;
                self.refresh();
            }
        }
        if let Some(anim) = &self.download_collapse_anim {
            if anim.start_time.elapsed().as_millis() >= anim.duration as u128 {
                self.download_collapse_anim = None;
                self.refresh();
            }
        }
    }

    fn ensure_download_timer(&self) {
        let panel_animating = self.download_panel.is_some()
            && (self.download_panel_reveal - self.download_panel_reveal_target).abs() > 0.005;
        let needs_timer = panel_animating
            || self.download_toast.is_some()
            || self.bookmark_toast.is_some()
            || self.download_removal_anim.is_some()
            || self.download_collapse_anim.is_some()
            || self.downloads.iter().any(|download| {
                download.state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS
                    || download
                        .completed_at
                        .map(|at| at.elapsed().as_millis() < 900)
                        .unwrap_or(false)
                    || download
                        .cancelled_at
                        .map(|at| at.elapsed().as_millis() < 420)
                        .unwrap_or(false)
            });
        unsafe {
            if needs_timer {
                let _ = WindowsAndMessaging::SetTimer(Some(self.hwnd), DOWNLOAD_TIMER_ID, 16, None);
            } else {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), DOWNLOAD_TIMER_ID);
            }
        }
    }

    fn poll_downloads(&mut self) {
        let active: Vec<(usize, ICoreWebView2DownloadOperation)> = self
            .downloads
            .iter()
            .filter(|download| download.state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS)
            .filter_map(|download| {
                download
                    .operation
                    .as_ref()
                    .map(|operation| (download.id, operation.clone()))
            })
            .collect();
        for (id, operation) in active {
            self.update_download_from_operation(id, &operation);
        }
        self.ensure_download_timer();
    }

    fn download_progress(&self, download: &DownloadItem) -> f32 {
        if download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
            return 1.0;
        }
        if download.total_bytes <= 0 {
            return 0.0;
        }
        (download.received_bytes as f32 / download.total_bytes as f32).clamp(0.0, 1.0)
    }

    fn download_indicator_rects(&self) -> Vec<(Option<usize>, RECT)> {
        if self.sidebar_width() <= 92 || self.downloads.is_empty() {
            return Vec::new();
        }
        let settings = self.settings_rect();
        let mut rects = Vec::new();
        let mut x = settings.right + 14;
        let y = settings.top;
        if self.downloads.len() > 3 {
            return vec![(
                None,
                RECT {
                    left: x,
                    top: y,
                    right: (x + 82).min(self.sidebar_width() - 12),
                    bottom: y + 32,
                },
            )];
        }
        let visible_count = self.downloads.len();
        for download in self.downloads.iter().take(visible_count) {
            rects.push((
                Some(download.id),
                RECT {
                    left: x,
                    top: y,
                    right: x + 32,
                    bottom: y + 32,
                },
            ));
            x += 40;
        }
        rects
    }

    fn download_panel_rect(&self) -> Option<RECT> {
        self.download_panel?;
        if self.downloads.is_empty() || self.sidebar_width() <= 92 {
            return None;
        }
        let settings = self.settings_rect();
        let rows = match self.download_panel {
            Some(DownloadPanelMode::Single(_)) => 1,
            Some(DownloadPanelMode::All) => self.downloads.len(),
            None => 0,
        };
        let height = 18 + rows as i32 * 58;
        Some(RECT {
            left: 12,
            top: (settings.top - height - 12).max(self.topbar_pushed_height() + 70),
            right: self.sidebar_width() - 12,
            bottom: settings.top - 12,
        })
    }

    fn download_panel_rows(&self) -> Vec<&DownloadItem> {
        match self.download_panel {
            Some(DownloadPanelMode::Single(id)) => self
                .downloads
                .iter()
                .filter(|download| download.id == id)
                .collect(),
            Some(DownloadPanelMode::All) => self.downloads.iter().collect(),
            None => Vec::new(),
        }
    }

    fn download_action_at(&self, x: i32, y: i32) -> Option<DownloadAction> {
        let panel = self.download_panel_rect()?;
        if !point_in_rect(x, y, panel) {
            return None;
        }
        let mut top = panel.top + 9;
        for download in self.download_panel_rows() {
            let row = RECT {
                left: panel.left + 12,
                top,
                right: panel.right - 12,
                bottom: top + 50,
            };
            let cancel = RECT {
                left: row.right - 22,
                top: row.top + 4,
                right: row.right,
                bottom: row.top + 26,
            };
            let open = RECT {
                left: row.right - 50,
                top: row.top + 4,
                right: row.right - 28,
                bottom: row.top + 26,
            };
            let pause = RECT {
                left: row.right - 78,
                top: row.top + 4,
                right: row.right - 56,
                bottom: row.top + 26,
            };
            if point_in_rect(x, y, cancel) {
                if download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED
                    || download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED
                {
                    return Some(DownloadAction::Delete(download.id));
                }
                return Some(DownloadAction::Cancel(download.id));
            }
            if download.state != COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED
                && point_in_rect(x, y, open)
            {
                return Some(DownloadAction::ShowInFolder(download.id));
            }
            if download.state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS
                && point_in_rect(x, y, pause)
            {
                return Some(DownloadAction::TogglePause(download.id));
            }
            top += 58;
        }
        None
    }

    fn run_download_action(&mut self, action: DownloadAction) {
        match action {
            DownloadAction::TogglePause(id) => {
                if let Some(download) = self.downloads.iter_mut().find(|item| item.id == id) {
                    if let Some(operation) = download.operation.as_ref() {
                        unsafe {
                            if download.paused {
                                let _ = operation.Resume();
                                download.paused = false;
                            } else if download.state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                                let _ = operation.Pause();
                                download.paused = true;
                            }
                        }
                    }
                }
            }
            DownloadAction::Cancel(id) => {
                if let Some(download) = self.downloads.iter_mut().find(|item| item.id == id) {
                    if let Some(operation) = download.operation.as_ref() {
                        unsafe {
                            let _ = operation.Cancel();
                        }
                    }
                    if !download.file_path.is_empty() {
                        let _ = fs::remove_file(&download.file_path);
                    }
                    download.state = COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED;
                    download.paused = false;
                    download.cancelled_at = Some(std::time::Instant::now());
                }
            }
            DownloadAction::ShowInFolder(id) => {
                if let Some(download) = self.downloads.iter().find(|item| item.id == id) {
                    open_in_file_explorer(&download.file_path);
                }
            }
            DownloadAction::Delete(id) => {
                let removed_index = self.downloads.iter().position(|item| item.id == id);
                let old_count = self.downloads.len();
                let mut cached = None;
                if let Some(download) = self.downloads.iter().find(|item| item.id == id) {
                    if !download.file_path.is_empty() {
                        let _ = fs::remove_file(&download.file_path);
                    }
                    cached = Some((
                        self.download_progress(download),
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                        download.completed_at,
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                        download.cancelled_at,
                    ));
                }
                self.downloads.retain(|item| item.id != id);
                if self.download_panel == Some(DownloadPanelMode::Single(id))
                    || self.downloads.is_empty()
                {
                    self.download_panel = None;
                }
                if let (Some(idx), Some((prog, compl, compl_at, cancelled, cancelled_at))) =
                    (removed_index, cached)
                {
                    if old_count >= 1 && old_count <= 4 {
                        self.download_removal_anim = Some(DownloadRemovalAnim {
                            start_time: std::time::Instant::now(),
                            duration: 180,
                            removed_id: id,
                            removed_index: idx,
                            old_count,
                            removed_progress: prog,
                            removed_completed: compl,
                            removed_completed_at: compl_at,
                            removed_cancelled: cancelled,
                            removed_cancelled_at: cancelled_at,
                        });
                        self.ensure_download_timer();
                    }
                }
            }
        }
        self.ensure_download_timer();
        self.refresh();
    }

    fn update_tab_title(&mut self, tab_id: usize, title: String) {
        let mut visited_update = None;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if tab.is_sleeping {
                return;
            }
            let trimmed = title.trim();
            if !trimmed.is_empty() {
                tab.title = trimmed.to_string();
                if let Some(entry) = tab.history.get_mut(tab.history_cursor) {
                    entry.title = tab.title.clone();
                }
                if !tab.url.trim().is_empty() && !tab.url.starts_with("aster:") {
                    visited_update = Some((tab.url.clone(), tab.title.clone()));
                }
            }
        }
        if let Some((url, title)) = visited_update {
            let norm_url = normalize_url_for_dedup(&url);
            if let Some(site) = self
                .visited_sites
                .iter_mut()
                .find(|item| normalize_url_for_dedup(&item.url) == norm_url)
            {
                site.title = title;
            }
        }
        self.save_state();
        self.refresh();
    }

    fn set_tab_loading(&mut self, tab_id: usize, is_loading: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if tab.is_sleeping {
                return;
            }
            tab.is_loading = is_loading;
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            };
        }
        let any_loading = self.tabs.iter().any(|t| t.is_loading);
        unsafe {
            if any_loading {
                let _ = WindowsAndMessaging::SetTimer(Some(self.hwnd), LOADING_TIMER_ID, 16, None);
            } else {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), LOADING_TIMER_ID);
            }
        }
    }

    fn update_tab_url(&mut self, tab_id: usize, url: String) {
        if self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.is_sleeping)
            .unwrap_or(false)
        {
            return;
        }
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
                suggestion = Some((tab.url.clone(), tab.title.clone()));
            }
            if Some(index) == active_index {
                if tab.unloaded {
                    set_window_text(self.address_hwnd, "");
                } else {
                    set_window_text(self.address_hwnd, &tab.url);
                }
            }
        }
        if let Some((url, title)) = suggestion {
            self.remember_suggestion(url, title);
        }
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
            let url = tab.url.clone();
            let favicon_uri = tab.favicon_uri.clone();
            self.request_favicon_for_url(&url, Some(&favicon_uri));
        }
        self.save_state();
        self.refresh();
    }

    fn update_tab_favicon_uri(&mut self, tab_id: usize, favicon_uri: String) {
        let mut page_url = None;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if tab.is_sleeping {
                return;
            }
            tab.favicon_uri = favicon_uri;
            page_url = Some(tab.url.clone());
        }
        if let Some(url) = page_url {
            let favicon_uri = self
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .map(|tab| tab.favicon_uri.clone())
                .unwrap_or_default();
            self.request_favicon_for_url(&url, Some(&favicon_uri));
        }
        self.refresh();
    }

    fn update_tab_favicon_bitmap(&mut self, tab_id: usize, favicon: FaviconBitmap) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if tab.is_sleeping {
                return;
            }
            tab.favicon_bitmap = Some(favicon);
        }
        self.refresh();
    }

    fn request_missing_favicons(&mut self) {
        let tab_requests: Vec<(String, String)> = self
            .tabs
            .iter()
            .filter(|tab| tab.favicon_bitmap.is_none())
            .map(|tab| (tab.url.clone(), tab.favicon_uri.clone()))
            .collect();
        for (url, favicon_uri) in tab_requests {
            self.request_favicon_for_url(&url, Some(&favicon_uri));
        }
        let history_urls: Vec<String> = self
            .visited_sites
            .iter()
            .take(80)
            .map(|site| site.url.clone())
            .collect();
        for url in history_urls {
            self.request_favicon_for_url(&url, None);
        }
    }

    fn request_command_favicons(&mut self) {
        let urls: Vec<String> = self
            .command_suggestions()
            .into_iter()
            .take(16)
            .map(|(_, _, url)| url)
            .collect();
        for url in urls {
            self.request_favicon_for_url(&url, None);
        }
    }

    fn request_favicon_for_url(&mut self, url: &str, known_favicon_uri: Option<&str>) {
        if url.trim().is_empty() || url.starts_with("aster:") || url == "about:blank" {
            return;
        }
        let host = display_host(url);
        if host.is_empty()
            || self.favicon_cache.contains_key(&host)
            || self.pending_favicon_hosts.iter().any(|pending| pending == &host)
        {
            return;
        }
        let candidates = favicon_candidate_urls(url, known_favicon_uri);
        if candidates.is_empty() {
            return;
        }
        self.pending_favicon_hosts.push(host.clone());
        let hwnd_raw = self.hwnd.0 as isize;
        let page_url = url.to_string();
        std::thread::spawn(move || {
            let path = download_first_favicon(&candidates, &page_url).unwrap_or_default();
            let result = Box::new(FaviconFetchResult { host, path });
            let ptr = Box::into_raw(result);
            let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
            let posted = unsafe {
                WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    FAVICON_FETCHED_MSG,
                    WPARAM(ptr as usize),
                    LPARAM(0),
                )
                .is_ok()
            };
            if !posted {
                unsafe {
                    drop(Box::from_raw(ptr));
                }
            }
        });
    }

    fn complete_favicon_fetch(&mut self, result: FaviconFetchResult) {
        self.pending_favicon_hosts
            .retain(|host| host != &result.host);
        if result.path.is_empty() {
            return;
        }
        if self.favicon_cache.contains_key(&result.host) {
            return;
        }
        if let Some(bitmap) = decode_favicon_file(&result.path) {
            self.favicon_cache.insert(result.host, bitmap);
            self.refresh();
        }
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

    fn remember_suggestion(&mut self, url: String, title: String) {
        let value = url.trim();
        if value.is_empty() || value == "about:blank" || value.starts_with("aster:") {
            return;
        }
        let now = current_timestamp();
        let norm_value = normalize_url_for_dedup(value);
        let title = if title.trim().is_empty() || title == "New Tab" {
            label_for_url(value)
        } else {
            title.trim().to_string()
        };
        if let Some(site) = self
            .visited_sites
            .iter_mut()
            .find(|item| normalize_url_for_dedup(&item.url) == norm_value)
        {
            site.visit_count += 1;
            site.last_visit_time = now;
            site.title = title;
            if value.len() < site.url.len() {
                site.url = value.to_string();
            }
        } else {
            self.visited_sites.push(VisitedSite {
                title,
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
        let visible_tab_ids = self.visible_tab_ids_for_index(index);
        let mut reloads: Vec<(usize, String)> = Vec::new();
        for tab in &mut self.tabs {
            let visible = tab.workspace_id == workspace_id && visible_tab_ids.contains(&tab.id);
            if wake_up && visible {
                if tab.is_sleeping {
                    let url_to_load = tab.pinned_url.clone().unwrap_or_else(|| tab.url.clone());
                    reloads.push((tab.id, url_to_load));
                    tab.is_sleeping = false;
                }
                tab.unloaded = false;
            }
            unsafe {
                let _ = WindowsAndMessaging::ShowWindow(
                    tab.child_hwnd,
                    if visible && !tab.unloaded {
                        WindowsAndMessaging::SW_SHOW
                    } else {
                        WindowsAndMessaging::SW_HIDE
                    },
                );
                let _ = tab.controller.SetIsVisible(visible && !tab.unloaded);
            }
        }
        if let Some(group_id) = self.tabs[index].split_group_id {
            if let Some(group) = self
                .split_groups
                .iter_mut()
                .find(|group| group.id == group_id)
            {
                group.active_tab_id = self.tabs[index].id;
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
        for (tab_id, url_to_load) in reloads {
            let Some(tab_index) = self.tab_index_by_id(tab_id) else {
                continue;
            };
            let tab = &self.tabs[tab_index];
            let url_w = to_wide(&url_to_load);
            unsafe {
                let _ = tab.webview.Navigate(PCWSTR(url_w.as_ptr()));
            }
        }
        self.save_state();
        self.refresh();
        self.ensure_hover_detect_timer();
    }

    fn switch_tab_above(&mut self) {
        let tabs = self.active_workspace_tabs();
        if tabs.len() <= 1 {
            return;
        }
        if let Some(active_idx) = self.active_tab_index() {
            if let Some(pos) = tabs.iter().position(|&idx| idx == active_idx) {
                let next_pos = if pos == 0 { tabs.len() - 1 } else { pos - 1 };
                self.switch_to(tabs[next_pos], true);
            }
        }
    }

    fn switch_tab_below(&mut self) {
        let tabs = self.active_workspace_tabs();
        if tabs.len() <= 1 {
            return;
        }
        if let Some(active_idx) = self.active_tab_index() {
            if let Some(pos) = tabs.iter().position(|&idx| idx == active_idx) {
                let next_pos = (pos + 1) % tabs.len();
                self.switch_to(tabs[next_pos], true);
            }
        }
    }

    fn ensure_hover_detect_timer(&mut self) {
        if (self.sidebar_mode == SidebarMode::Hidden && !self.animating_sidebar)
            || (self.topbar_mode == SidebarMode::Hidden && !self.animating_topbar)
        {
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
        let closing_tab_id = self.tabs[index].id;
        self.detach_tab_from_split_by_id(closing_tab_id);

        if !self.tabs[index].pinned {
            let tab = &self.tabs[index];
            self.closed_tabs.push(ClosedTab {
                url: tab.url.clone(),
                title: tab.title.clone(),
                workspace_id: tab.workspace_id,
                folder_id: tab.folder_id,
            });
            if self.closed_tabs.len() > 100 {
                self.closed_tabs.remove(0);
            }
        }

        let workspace_id = self.tabs[index].workspace_id;

        if self.tabs[index].pinned {
            let tab = &mut self.tabs[index];
            tab.unloaded = true;
            tab.is_sleeping = true;
            if let Some(pinned_url) = tab.pinned_url.clone() {
                tab.url = pinned_url;
            }
            let blank_w = to_wide("about:blank");
            unsafe {
                let _ = tab.webview.Navigate(PCWSTR(blank_w.as_ptr()));
            }
            unsafe {
                let _ = tab.controller.SetIsVisible(false);
                let _ =
                    WindowsAndMessaging::ShowWindow(tab.child_hwnd, WindowsAndMessaging::SW_HIDE);
            }
            if self.active == index {
                let next = self
                    .tabs
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

    fn reopen_closed_tab(&mut self) {
        if let Some(closed) = self.closed_tabs.pop() {
            let mut target_workspace = closed.workspace_id;
            if !self.workspaces.iter().any(|w| w.id == target_workspace) {
                target_workspace = self.active_workspace;
            }
            let mut target_folder = closed.folder_id;
            if let Some(f_id) = target_folder {
                if !self.folders.iter().any(|f| f.id == f_id) {
                    target_folder = None;
                }
            }
            let _ = self.create_tab_in_workspace(
                &closed.url,
                target_workspace,
                target_folder,
                false,
                true,
                Some(closed.title),
            );
        }
    }

    fn reopen_closed_tab_at(&mut self, recent_index: usize) {
        let Some(index) = self.closed_tabs.len().checked_sub(1 + recent_index) else {
            return;
        };
        let closed = self.closed_tabs.remove(index);
        let mut target_workspace = closed.workspace_id;
        if !self.workspaces.iter().any(|w| w.id == target_workspace) {
            target_workspace = self.active_workspace;
        }
        let mut target_folder = closed.folder_id;
        if let Some(f_id) = target_folder {
            if !self.folders.iter().any(|f| f.id == f_id) {
                target_folder = None;
            }
        }
        let _ = self.create_tab_in_workspace(
            &closed.url,
            target_workspace,
            target_folder,
            false,
            true,
            Some(closed.title),
        );
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

    fn open_settings_page(&mut self) {
        let _ = self.create_tab("aster:settings");
    }

    fn open_history_page(&mut self) {
        let _ = self.create_tab("aster:history");
    }

    fn open_extensions_page(&mut self) {
        let _ = self.create_tab("aster:extensions");
        self.refresh_extensions();
    }

    fn open_extension_store(&mut self) {
        let _ = self.create_tab(CHROME_WEB_STORE_URL);
    }

    fn navigate_active(&mut self, url: &str) {
        let Some(index) = self.active_tab_index() else {
            let _ = self.create_tab(url);
            return;
        };
        if url == "aster:settings" {
            self.load_settings_page(index);
            return;
        }
        if url == "aster:history" {
            self.load_history_page(index);
            return;
        }
        if url == "aster:extensions" {
            self.load_extensions_page(index);
            self.refresh_extensions();
            return;
        }
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

    fn load_extensions_page(&mut self, index: usize) {
        let html = extensions_page_html(
            self.dominant_color,
            self.secondary_color,
            self.accent_color,
            &self.extensions,
            self.extension_notice.as_deref().unwrap_or(""),
        );
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.url = "aster:extensions".to_string();
            tab.title = "Aster Extensions".to_string();
            tab.favicon_bitmap = render_glyph_favicon(18, 0xECAA, &self.fonts.icon, COLOR_ACCENT);
            unsafe {
                let html = CoTaskMemPWSTR::from(html.as_str());
                let _ = tab.webview.NavigateToString(*html.as_ref().as_pcwstr());
            }
            set_window_text(self.address_hwnd, "aster:extensions");
        }
        self.save_state();
        self.refresh();
    }

    fn load_history_page(&mut self, index: usize) {
        let html = history_page_html(
            self.dominant_color,
            self.secondary_color,
            self.accent_color,
            self.history_sort_mode,
            self.history_visit_sort_desc,
            &self.visited_sites,
        );
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.url = "aster:history".to_string();
            tab.title = "Aster History".to_string();
            tab.favicon_bitmap = render_glyph_favicon(18, 0xE81C, &self.fonts.icon, COLOR_ACCENT);
            unsafe {
                let html = CoTaskMemPWSTR::from(html.as_str());
                let _ = tab.webview.NavigateToString(*html.as_ref().as_pcwstr());
            }
            set_window_text(self.address_hwnd, "aster:history");
        }
        self.save_state();
        self.refresh();
    }

    fn reload_history_pages(&mut self) {
        let indexes: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| {
                if tab.url == "aster:history" {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();
        for index in indexes {
            self.load_history_page(index);
        }
    }

    fn reload_settings_pages(&mut self) {
        let indexes: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| {
                if tab.url == "aster:settings" {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();
        for index in indexes {
            self.load_settings_page(index);
        }
    }

    fn reload_extensions_pages(&mut self) {
        let indexes: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| {
                if tab.url == "aster:extensions" {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();
        for index in indexes {
            self.load_extensions_page(index);
        }
    }

    fn portable_bookmarks(&self) -> Vec<PortableBookmark> {
        self.bookmarks
            .iter()
            .map(|bookmark| PortableBookmark {
                title: bookmark.title.clone(),
                url: bookmark.url.clone(),
                folder: bookmark
                    .folder_id
                    .and_then(|id| self.bookmark_folders.iter().find(|folder| folder.id == id))
                    .map(|folder| folder.name.clone())
                    .unwrap_or_default(),
                tags: bookmark.tags.clone(),
                created_at: bookmark.created_at,
            })
            .collect()
    }

    fn portable_history(&self) -> Vec<PortableHistoryEntry> {
        self.visited_sites
            .iter()
            .map(|site| PortableHistoryEntry {
                title: site.title.clone(),
                url: site.url.clone(),
                visit_count: site.visit_count.max(1),
                last_visit_time: site.last_visit_time,
            })
            .collect()
    }

    fn cookie_manager(&self) -> Option<ICoreWebView2CookieManager> {
        let tab = self.tabs.get(self.active).or_else(|| self.tabs.first())?;
        unsafe {
            let webview2 = tab.webview.cast::<ICoreWebView2_2>().ok()?;
            webview2.CookieManager().ok()
        }
    }

    fn export_cookies(&self) -> Vec<PortableCookie> {
        let Some(manager) = self.cookie_manager() else {
            return Vec::new();
        };
        let cookies = std::rc::Rc::new(RefCell::new(Vec::new()));
        let output = cookies.clone();
        let result = GetCookiesCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                let uri = CoTaskMemPWSTR::from("");
                manager
                    .GetCookies(*uri.as_ref().as_pcwstr(), &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, list| {
                error_code?;
                if let Some(list) = list {
                    unsafe {
                        let mut count = 0u32;
                        let _ = list.Count(&mut count);
                        for index in 0..count {
                            if let Ok(cookie) = list.GetValueAtIndex(index) {
                                if let Some(portable) = portable_cookie_from_webview(&cookie) {
                                    output.borrow_mut().push(portable);
                                }
                            }
                        }
                    }
                }
                Ok(())
            }),
        );
        if result.is_err() {
            return Vec::new();
        }
        let exported = cookies.borrow().clone();
        exported
    }

    fn aster_export_json(&self) -> String {
        let export = AsterDataExport {
            app: "Aster".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            exported_at: current_timestamp(),
            bookmarks: self.portable_bookmarks(),
            history: self.portable_history(),
            cookies: self.export_cookies(),
        };
        serde_json::to_string_pretty(&export).unwrap_or_else(|_| "{}".to_string())
    }

    fn send_settings_export(&self, kind: &str) {
        let (mime, payload) = match kind {
            "aster" => ("application/json", self.aster_export_json()),
            "bookmarks" => ("text/html", self.bookmarks_export_html()),
            _ => return,
        };
        let script = format!(
            "window.asterReceiveExport && window.asterReceiveExport({}, {}, {});",
            js_string_literal(kind),
            js_string_literal(mime),
            js_string_literal(&payload)
        );
        unsafe {
            let js = CoTaskMemPWSTR::from(script.as_str());
            for tab in self.tabs.iter().filter(|tab| tab.url == "aster:settings") {
                let _ = tab.webview.ExecuteScript(
                    *js.as_ref().as_pcwstr(),
                    &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
                );
            }
        }
    }

    fn active_extension_profile(&self) -> Option<ICoreWebView2Profile7> {
        let tab = self.tabs.get(self.active).or_else(|| self.tabs.first())?;
        unsafe {
            let webview13 = tab.webview.cast::<ICoreWebView2_13>().ok()?;
            webview13.Profile().ok()?.cast::<ICoreWebView2Profile7>().ok()
        }
    }

    fn refresh_extensions(&self) {
        let Some(profile) = self.active_extension_profile() else {
            return;
        };
        let hwnd = self.hwnd;
        unsafe {
            let _ = profile.GetBrowserExtensions(
                &ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                    move |error_code, list| {
                        let mut extensions = Vec::new();
                        if error_code.is_ok() {
                            if let Some(list) = list {
                                let mut count = 0u32;
                                let _ = list.Count(&mut count);
                                for index in 0..count {
                                    if let Ok(extension) = list.GetValueAtIndex(index) {
                                        extensions.push(extension_info(&extension));
                                    }
                                }
                            }
                        }
                        with_app(hwnd, |app| {
                            app.extensions = extensions;
                            if app.extension_notice.as_deref()
                                == Some("Refreshing extensions...")
                            {
                                app.extension_notice =
                                    Some("Extension list refreshed.".to_string());
                            }
                            app.reload_extensions_pages();
                            app.refresh();
                        });
                        Ok(())
                    },
                )),
            );
        }
    }

    fn add_extension_from_path(&mut self, path: &str) {
        let path = path.trim().trim_matches('"');
        if path.is_empty() {
            self.extension_notice = Some("Enter an unpacked extension folder path.".to_string());
            self.reload_extensions_pages();
            return;
        }
        let folder = PathBuf::from(path);
        if !folder.join("manifest.json").exists() {
            self.extension_notice =
                Some("That folder does not contain manifest.json.".to_string());
            self.reload_extensions_pages();
            return;
        }
        let Some(profile) = self.active_extension_profile() else {
            self.extension_notice =
                Some("Extensions are not available in this WebView2 runtime.".to_string());
            self.reload_extensions_pages();
            return;
        };
        self.extension_notice = Some("Loading extension...".to_string());
        self.reload_extensions_pages();
        let hwnd = self.hwnd;
        let path = folder.to_string_lossy().into_owned();
        unsafe {
            let wide = CoTaskMemPWSTR::from(path.as_str());
            let _ = profile.AddBrowserExtension(
                *wide.as_ref().as_pcwstr(),
                &ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                    move |error_code, extension| {
                        with_app(hwnd, |app| {
                            app.extension_notice = if error_code.is_ok() {
                                let name = extension
                                    .as_ref()
                                    .map(extension_info)
                                    .map(|info| info.name)
                                    .unwrap_or_else(|| "Extension".to_string());
                                Some(format!("{name} loaded."))
                            } else {
                                Some("Extension could not be loaded.".to_string())
                            };
                            app.refresh_extensions();
                        });
                        Ok(())
                    },
                )),
            );
        }
    }

    fn with_extension_by_id<F>(&self, id: &str, action: F)
    where
        F: FnOnce(ICoreWebView2BrowserExtension) + 'static,
    {
        let Some(profile) = self.active_extension_profile() else {
            return;
        };
        let wanted = id.to_string();
        let action = RefCell::new(Some(action));
        unsafe {
            let _ = profile.GetBrowserExtensions(
                &ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                    move |error_code, list| {
                        if error_code.is_ok() {
                            if let Some(list) = list {
                                let mut count = 0u32;
                                let _ = list.Count(&mut count);
                                for index in 0..count {
                                    if let Ok(extension) = list.GetValueAtIndex(index) {
                                        if extension_info(&extension).id == wanted {
                                            if let Some(action) = action.borrow_mut().take() {
                                                action(extension);
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(())
                    },
                )),
            );
        }
    }

    fn set_extension_enabled(&mut self, id: &str, enabled: bool) {
        let id_for_notice = id.to_string();
        let hwnd = self.hwnd;
        self.extension_notice = Some(if enabled {
            "Enabling extension...".to_string()
        } else {
            "Disabling extension...".to_string()
        });
        self.reload_extensions_pages();
        self.with_extension_by_id(id, move |extension| unsafe {
            let _ = extension.Enable(
                enabled,
                &BrowserExtensionEnableCompletedHandler::create(Box::new(move |error_code| {
                    with_app(hwnd, |app| {
                        app.extension_notice = if error_code.is_ok() {
                            Some(if enabled {
                                "Extension enabled.".to_string()
                            } else {
                                "Extension disabled.".to_string()
                            })
                        } else {
                            Some(format!("Could not update extension {id_for_notice}."))
                        };
                        app.refresh_extensions();
                    });
                    Ok(())
                })),
            );
        });
    }

    fn remove_extension(&mut self, id: &str) {
        let id_for_notice = id.to_string();
        let hwnd = self.hwnd;
        self.extension_notice = Some("Removing extension...".to_string());
        self.reload_extensions_pages();
        self.with_extension_by_id(id, move |extension| unsafe {
            let _ = extension.Remove(&BrowserExtensionRemoveCompletedHandler::create(Box::new(
                move |error_code| {
                    with_app(hwnd, |app| {
                        app.extension_notice = if error_code.is_ok() {
                            Some("Extension removed.".to_string())
                        } else {
                            Some(format!("Could not remove extension {id_for_notice}."))
                        };
                        app.refresh_extensions();
                    });
                    Ok(())
                },
            )));
        });
    }

    fn bookmarks_export_html(&self) -> String {
        let mut html = String::from(
            "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
        );
        for bookmark in &self.bookmarks {
            html.push_str(&format!(
                "    <DT><A HREF=\"{}\" ADD_DATE=\"{}\">{}</A>\n",
                html_escape_attr(&bookmark.url),
                bookmark.created_at,
                html_escape_text(&bookmark.title)
            ));
        }
        html.push_str("</DL><p>\n");
        html
    }

    fn import_uploaded_files(&mut self, raw: &str) -> ImportStats {
        let uploads: Vec<ImportUploadFile> = match serde_json::from_str(raw) {
            Ok(files) => files,
            Err(error) => {
                let mut stats = ImportStats::default();
                stats
                    .errors
                    .push(format!("Could not read dropped files: {error}"));
                return stats;
            }
        };
        let mut decoded = Vec::new();
        for upload in uploads {
            match decode_upload_content(&upload.content) {
                Ok(bytes) => decoded.push((upload, bytes)),
                Err(error) => {
                    let mut stats = ImportStats::default();
                    stats
                        .errors
                        .push(format!("{} could not be decoded: {error}", upload.name));
                    return stats;
                }
            }
        }

        let chromium_key = decoded
            .iter()
            .find_map(|(_, bytes)| extract_chromium_profile_key(bytes));
        let mut total = ImportStats::default();
        for (upload, bytes) in decoded {
            if extract_chromium_profile_key(&bytes).is_some() {
                continue;
            }
            let mut stats = if looks_like_sqlite(&upload.name, &bytes) {
                self.import_sqlite_data(&upload.name, &upload.path, &bytes, chromium_key.as_deref())
            } else if looks_like_html(&upload.name, &bytes) {
                self.import_bookmarks_html(&bytes)
            } else {
                self.import_json_data(&upload.name, &bytes)
            };
            if stats.bookmarks == 0
                && stats.history == 0
                && stats.cookies == 0
                && stats.errors.is_empty()
            {
                stats.errors.push(format!(
                    "{} did not contain supported browser data",
                    upload.name
                ));
            }
            total.absorb(stats);
        }
        self.save_state();
        self.request_missing_favicons();
        total
    }

    fn import_json_data(&mut self, name: &str, bytes: &[u8]) -> ImportStats {
        let mut stats = ImportStats::default();
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            stats.errors.push(format!("{name} is not valid JSON"));
            return stats;
        };

        if value.get("bookmarks").is_some()
            || value.get("history").is_some()
            || value.get("cookies").is_some()
        {
            match serde_json::from_value::<AsterDataExport>(value.clone()) {
                Ok(export) => {
                    stats.absorb(self.merge_portable_bookmarks(export.bookmarks));
                    stats.absorb(self.merge_portable_history(export.history));
                    stats.absorb(self.import_portable_cookies(export.cookies));
                    return stats;
                }
                Err(error) => {
                    stats.errors.push(format!(
                        "{name} looked like an Aster export but failed: {error}"
                    ));
                    return stats;
                }
            }
        }

        let mut bookmarks = Vec::new();
        collect_bookmarks_from_json(&value, "", &mut bookmarks);
        if !bookmarks.is_empty() {
            stats.absorb(self.merge_portable_bookmarks(bookmarks));
        }

        let mut history = Vec::new();
        collect_history_from_json(&value, &mut history);
        if !history.is_empty() {
            stats.absorb(self.merge_portable_history(history));
        }

        let mut cookies = Vec::new();
        collect_cookies_from_json(&value, &mut cookies);
        if !cookies.is_empty() {
            stats.absorb(self.import_portable_cookies(cookies));
        }
        stats
    }

    fn import_bookmarks_html(&mut self, bytes: &[u8]) -> ImportStats {
        let Ok(raw) = std::str::from_utf8(bytes) else {
            let mut stats = ImportStats::default();
            stats
                .errors
                .push("Bookmark HTML was not UTF-8 text".to_string());
            return stats;
        };
        self.merge_portable_bookmarks(parse_bookmark_html(raw))
    }

    fn import_sqlite_data(
        &mut self,
        name: &str,
        path_hint: &str,
        bytes: &[u8],
        chromium_key: Option<&[u8]>,
    ) -> ImportStats {
        let mut stats = ImportStats::default();
        let temp_path = sqlite_temp_path(name, path_hint);
        if let Err(error) = fs::write(&temp_path, bytes) {
            stats.errors.push(format!(
                "{name} could not be staged for SQLite import: {error}"
            ));
            return stats;
        }

        let result = (|| -> std::result::Result<ImportStats, String> {
            let connection = Connection::open(&temp_path).map_err(|error| error.to_string())?;
            let tables = sqlite_table_names(&connection);
            let mut found = ImportStats::default();
            if tables.iter().any(|table| table == "urls") {
                found.absorb(self.import_chromium_history(&connection, name));
            }
            if tables.iter().any(|table| table == "moz_places") {
                found.absorb(self.import_firefox_history(&connection, name));
            }
            if tables.iter().any(|table| table == "cookies") {
                found.absorb(self.import_chromium_cookies(&connection, name, chromium_key));
            }
            if tables.iter().any(|table| table == "moz_cookies") {
                found.absorb(self.import_firefox_cookies(&connection, name));
            }
            Ok(found)
        })();

        let _ = fs::remove_file(&temp_path);
        match result {
            Ok(found) => found,
            Err(error) => {
                stats.errors.push(format!(
                    "{name} could not be parsed as browser SQLite: {error}"
                ));
                stats
            }
        }
    }

    fn import_chromium_history(&mut self, connection: &Connection, name: &str) -> ImportStats {
        let mut stats = ImportStats::default();
        let mut statement = match connection.prepare(
            "SELECT url, COALESCE(title,''), COALESCE(visit_count,1), COALESCE(last_visit_time,0) FROM urls",
        ) {
            Ok(statement) => statement,
            Err(error) => {
                stats
                    .errors
                    .push(format!("{name} history table could not be read: {error}"));
                return stats;
            }
        };
        let rows = statement.query_map([], |row| {
            Ok(PortableHistoryEntry {
                url: row.get::<_, String>(0)?,
                title: row.get::<_, String>(1)?,
                visit_count: row.get::<_, u32>(2).unwrap_or(1),
                last_visit_time: chrome_time_to_unix_secs(row.get::<_, i64>(3).unwrap_or(0)),
            })
        });
        match rows {
            Ok(rows) => {
                let history = rows.filter_map(|row| row.ok()).collect();
                stats.absorb(self.merge_portable_history(history));
            }
            Err(error) => stats
                .errors
                .push(format!("{name} history rows could not be read: {error}")),
        }
        stats
    }

    fn import_firefox_history(&mut self, connection: &Connection, name: &str) -> ImportStats {
        let mut stats = ImportStats::default();
        let mut statement = match connection.prepare(
            "SELECT url, COALESCE(title,''), COALESCE(visit_count,1), COALESCE(last_visit_date,0) FROM moz_places WHERE url IS NOT NULL",
        ) {
            Ok(statement) => statement,
            Err(error) => {
                stats
                    .errors
                    .push(format!("{name} places table could not be read: {error}"));
                return stats;
            }
        };
        let rows = statement.query_map([], |row| {
            let last_visit = row.get::<_, i64>(3).unwrap_or(0);
            Ok(PortableHistoryEntry {
                url: row.get::<_, String>(0)?,
                title: row.get::<_, String>(1)?,
                visit_count: row.get::<_, u32>(2).unwrap_or(1),
                last_visit_time: if last_visit > 0 {
                    (last_visit / 1_000_000).max(0) as u64
                } else {
                    current_timestamp()
                },
            })
        });
        match rows {
            Ok(rows) => {
                let history = rows.filter_map(|row| row.ok()).collect();
                stats.absorb(self.merge_portable_history(history));
            }
            Err(error) => stats
                .errors
                .push(format!("{name} places rows could not be read: {error}")),
        }
        stats
    }

    fn import_chromium_cookies(
        &mut self,
        connection: &Connection,
        name: &str,
        chromium_key: Option<&[u8]>,
    ) -> ImportStats {
        let mut stats = ImportStats::default();
        let mut statement = match connection.prepare(
            "SELECT host_key, name, path, COALESCE(expires_utc,0), COALESCE(is_secure,0), COALESCE(is_httponly,0), COALESCE(samesite,0), COALESCE(value,''), encrypted_value FROM cookies",
        ) {
            Ok(statement) => statement,
            Err(error) => {
                stats
                    .errors
                    .push(format!("{name} cookies table could not be read: {error}"));
                return stats;
            }
        };
        let rows = statement.query_map([], |row| {
            let encrypted = row.get::<_, Vec<u8>>(8).unwrap_or_default();
            let mut value = row.get::<_, String>(7).unwrap_or_default();
            if value.is_empty() {
                value = chromium_key
                    .and_then(|key| decrypt_chromium_cookie(&encrypted, key).ok())
                    .or_else(|| {
                        dpapi_decrypt(&encrypted)
                            .ok()
                            .and_then(|bytes| String::from_utf8(bytes).ok())
                    })
                    .unwrap_or_default();
            }
            Ok(PortableCookie {
                domain: row.get::<_, String>(0)?,
                name: row.get::<_, String>(1)?,
                path: row.get::<_, String>(2).unwrap_or_else(|_| "/".to_string()),
                expires: Some(chrome_time_to_unix_secs(row.get::<_, i64>(3).unwrap_or(0)) as f64),
                secure: row.get::<_, i32>(4).unwrap_or(0) != 0,
                http_only: row.get::<_, i32>(5).unwrap_or(0) != 0,
                same_site: same_site_from_chromium(row.get::<_, i32>(6).unwrap_or(0)),
                value,
            })
        });
        match rows {
            Ok(rows) => {
                let cookies = rows.filter_map(|row| row.ok()).collect();
                stats.absorb(self.import_portable_cookies(cookies));
            }
            Err(error) => stats
                .errors
                .push(format!("{name} cookie rows could not be read: {error}")),
        }
        if chromium_key.is_none() && stats.cookies == 0 {
            stats.errors.push(format!(
                "{name} contains encrypted Chromium cookies. Drop the matching Local State file with it to import account sessions."
            ));
        }
        stats
    }

    fn import_firefox_cookies(&mut self, connection: &Connection, name: &str) -> ImportStats {
        let mut stats = ImportStats::default();
        let mut statement = match connection.prepare(
            "SELECT host, name, value, path, COALESCE(expiry,0), COALESCE(isSecure,0), COALESCE(isHttpOnly,0), COALESCE(sameSite,0) FROM moz_cookies",
        ) {
            Ok(statement) => statement,
            Err(error) => {
                stats
                    .errors
                    .push(format!("{name} cookies table could not be read: {error}"));
                return stats;
            }
        };
        let rows = statement.query_map([], |row| {
            Ok(PortableCookie {
                domain: row.get::<_, String>(0)?,
                name: row.get::<_, String>(1)?,
                value: row.get::<_, String>(2)?,
                path: row.get::<_, String>(3).unwrap_or_else(|_| "/".to_string()),
                expires: Some(row.get::<_, i64>(4).unwrap_or(0).max(0) as f64),
                secure: row.get::<_, i32>(5).unwrap_or(0) != 0,
                http_only: row.get::<_, i32>(6).unwrap_or(0) != 0,
                same_site: same_site_from_firefox(row.get::<_, i32>(7).unwrap_or(0)),
            })
        });
        match rows {
            Ok(rows) => {
                let cookies = rows.filter_map(|row| row.ok()).collect();
                stats.absorb(self.import_portable_cookies(cookies));
            }
            Err(error) => stats
                .errors
                .push(format!("{name} cookie rows could not be read: {error}")),
        }
        stats
    }

    fn merge_portable_bookmarks(&mut self, bookmarks: Vec<PortableBookmark>) -> ImportStats {
        let mut stats = ImportStats::default();
        self.ensure_default_bookmark_folder();
        for bookmark in bookmarks {
            let url = normalize_address(&bookmark.url);
            if url.starts_with("aster:") || url.trim().is_empty() {
                stats.skipped += 1;
                continue;
            }
            if self.bookmark_index_for_url(&url).is_some() {
                stats.skipped += 1;
                continue;
            }
            let folder_id = self.folder_id_for_imported_bookmark(&bookmark.folder);
            let id = self.next_bookmark_id;
            self.next_bookmark_id += 1;
            let order = self.allocate_sidebar_order();
            self.bookmarks.push(Bookmark {
                id,
                folder_id,
                title: if bookmark.title.trim().is_empty() {
                    label_for_url(&url)
                } else {
                    bookmark.title
                },
                url,
                tags: bookmark.tags,
                created_at: if bookmark.created_at == 0 {
                    current_timestamp()
                } else {
                    bookmark.created_at
                },
                sidebar_order: order,
            });
            stats.bookmarks += 1;
        }
        stats
    }

    fn folder_id_for_imported_bookmark(&mut self, folder_name: &str) -> Option<usize> {
        let name = folder_name.trim();
        if name.is_empty() {
            return self.bookmark_folders.first().map(|folder| folder.id);
        }
        if let Some(folder) = self
            .bookmark_folders
            .iter()
            .find(|folder| folder.parent_id.is_none() && folder.name.eq_ignore_ascii_case(name))
        {
            return Some(folder.id);
        }
        let id = self.next_bookmark_folder_id;
        self.next_bookmark_folder_id += 1;
        let order = self.allocate_sidebar_order();
        self.bookmark_folders.push(BookmarkFolder {
            id,
            parent_id: None,
            name: name.to_string(),
            sidebar_order: order,
        });
        Some(id)
    }

    fn merge_portable_history(&mut self, history: Vec<PortableHistoryEntry>) -> ImportStats {
        let mut stats = ImportStats::default();
        for entry in history {
            let url = normalize_address(&entry.url);
            if url.trim().is_empty() || url.starts_with("aster:") {
                stats.skipped += 1;
                continue;
            }
            let norm = normalize_url_for_dedup(&url);
            let title = if entry.title.trim().is_empty() {
                label_for_url(&url)
            } else {
                entry.title
            };
            if let Some(existing) = self
                .visited_sites
                .iter_mut()
                .find(|site| normalize_url_for_dedup(&site.url) == norm)
            {
                existing.visit_count = existing.visit_count.max(entry.visit_count.max(1));
                existing.last_visit_time = existing.last_visit_time.max(entry.last_visit_time);
                if existing.title.trim().is_empty() {
                    existing.title = title;
                }
                stats.skipped += 1;
            } else {
                self.visited_sites.push(VisitedSite {
                    title,
                    url,
                    visit_count: entry.visit_count.max(1),
                    last_visit_time: if entry.last_visit_time == 0 {
                        current_timestamp()
                    } else {
                        entry.last_visit_time
                    },
                });
                stats.history += 1;
            }
        }
        if self.visited_sites.len() > 1000 {
            self.visited_sites.sort_by_key(|site| site.last_visit_time);
            let drain = self.visited_sites.len() - 1000;
            self.visited_sites.drain(0..drain);
        }
        stats
    }

    fn import_portable_cookies(&mut self, cookies: Vec<PortableCookie>) -> ImportStats {
        let mut stats = ImportStats::default();
        let Some(manager) = self.cookie_manager() else {
            if !cookies.is_empty() {
                stats
                    .errors
                    .push("No active browser profile was available for cookie import".to_string());
            }
            return stats;
        };
        unsafe {
            for portable in cookies {
                if portable.name.trim().is_empty()
                    || portable.domain.trim().is_empty()
                    || portable.value.is_empty()
                {
                    stats.skipped += 1;
                    continue;
                }
                let name = CoTaskMemPWSTR::from(portable.name.as_str());
                let value = CoTaskMemPWSTR::from(portable.value.as_str());
                let domain = CoTaskMemPWSTR::from(portable.domain.as_str());
                let path = CoTaskMemPWSTR::from(if portable.path.trim().is_empty() {
                    "/"
                } else {
                    portable.path.as_str()
                });
                match manager.CreateCookie(
                    *name.as_ref().as_pcwstr(),
                    *value.as_ref().as_pcwstr(),
                    *domain.as_ref().as_pcwstr(),
                    *path.as_ref().as_pcwstr(),
                ) {
                    Ok(cookie) => {
                        if let Some(expires) = portable.expires {
                            if expires > 0.0 {
                                let _ = cookie.SetExpires(expires);
                            }
                        }
                        let _ = cookie.SetIsSecure(portable.secure);
                        let _ = cookie.SetIsHttpOnly(portable.http_only);
                        let _ = cookie.SetSameSite(same_site_to_webview(&portable.same_site));
                        if manager.AddOrUpdateCookie(&cookie).is_ok() {
                            if is_account_cookie(&portable) {
                                stats.accounts += 1;
                            }
                            stats.cookies += 1;
                        } else {
                            stats.skipped += 1;
                        }
                    }
                    Err(error) => stats.errors.push(format!(
                        "Cookie {} for {} could not be created: {error}",
                        portable.name, portable.domain
                    )),
                }
            }
        }
        stats
    }

    fn load_settings_page(&mut self, index: usize) {
        let html = settings_page_html(
            self.dominant_color,
            self.secondary_color,
            self.accent_color,
            self.site_mode.label(),
            match self.startup_mode {
                StartupMode::HomePage => "home",
                StartupMode::LastSession => "last",
            },
            is_aster_default_browser(),
            self.import_export_notice.as_deref().unwrap_or(""),
        );
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.url = "aster:settings".to_string();
            tab.title = "Aster Settings".to_string();
            tab.favicon_bitmap = render_glyph_favicon(18, 0xE713, &self.fonts.icon, COLOR_ACCENT);
            unsafe {
                let html = CoTaskMemPWSTR::from(html.as_str());
                let _ = tab.webview.NavigateToString(*html.as_ref().as_pcwstr());
            }
            set_window_text(self.address_hwnd, "aster:settings");
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
                .map(|tab| if tab.unloaded { "" } else { tab.url.as_str() })
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
        self.request_command_favicons();
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
        let sidebar_order = self.allocate_sidebar_order();
        self.folders.push(Folder {
            id,
            workspace_id: self.active_workspace,
            parent_id: None,
            name: "New Folder".to_string(),
            collapsed: false,
            pinned: false,
            sidebar_order,
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

        let row_rect = self
            .sidebar_row_rects()
            .into_iter()
            .find_map(|(row, rect)| match row {
                SidebarRow::Folder(id) if id == folder_id => Some(rect),
                _ => None,
            });

        if let Some(rect) = row_rect {
            unsafe {
                let hinstance =
                    HINSTANCE(LibraryLoader::GetModuleHandleW(None).unwrap_or_default().0);
                let edit_hwnd = WindowsAndMessaging::CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("EDIT"),
                    w!(""),
                    WINDOW_STYLE(
                        WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | 0x0080, /* ES_AUTOHSCROLL */
                    ),
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

                    let _ = WindowsAndMessaging::PostMessageW(
                        Some(self.hwnd),
                        FOCUS_EDIT_MSG,
                        WPARAM(edit.0 as usize),
                        LPARAM(0),
                    );
                    self.renaming_edit = Some(edit);
                }
            }
        }
    }

    fn position_rename_edit(&self) {
        if let Some(edit_hwnd) = self.renaming_edit {
            if let Some(folder_id) = self.renaming_folder_id {
                let row_rect =
                    self.sidebar_row_rects()
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
        let y = self.topbar_y();
        (
            RECT {
                left: x,
                top: y + 16,
                right: x + 28,
                bottom: y + 44,
            },
            RECT {
                left: x + 38,
                top: y + 16,
                right: x + 66,
                bottom: y + 44,
            },
            RECT {
                left: x + 76,
                top: y + 16,
                right: x + 104,
                bottom: y + 44,
            },
        )
    }

    fn logo_rect(&self) -> RECT {
        let y = self.topbar_y();
        RECT {
            left: 12,
            top: y + 13,
            right: 42,
            bottom: y + 43,
        }
    }

    fn new_tab_rect(&self) -> RECT {
        let (_, _, reload) = self.top_button_rects();
        let y = self.topbar_y();
        RECT {
            left: reload.right + 10,
            top: y + 16,
            right: reload.right + 38,
            bottom: y + 44,
        }
    }

    fn extension_button_rect(&self) -> RECT {
        let (min_btn, _, _) = self.window_button_rects();
        let y = self.topbar_y();
        RECT {
            left: min_btn.left - 46,
            top: y + 16,
            right: min_btn.left - 18,
            bottom: y + 44,
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
            top: bottom - 236,
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

    fn settings_page_row_rect(&self) -> RECT {
        let row = self.history_page_row_rect();
        RECT {
            left: row.left,
            top: row.bottom + 8,
            right: row.right,
            bottom: row.bottom + 44,
        }
    }

    fn history_page_row_rect(&self) -> RECT {
        let row = self.extensions_page_row_rect();
        RECT {
            left: row.left,
            top: row.bottom + 8,
            right: row.right,
            bottom: row.bottom + 44,
        }
    }

    fn extension_store_row_rect(&self) -> RECT {
        let row = self.mode_row_rect();
        RECT {
            left: row.left,
            top: row.bottom + 8,
            right: row.right,
            bottom: row.bottom + 44,
        }
    }

    fn extensions_page_row_rect(&self) -> RECT {
        let row = self.extension_store_row_rect();
        RECT {
            left: row.left,
            top: row.bottom + 8,
            right: row.right,
            bottom: row.bottom + 44,
        }
    }

    fn mode_options_rect(&self) -> RECT {
        let row = self.mode_row_rect();
        let panel_width = 132;
        let right = self.sidebar_width() - 12;
        let left = right - panel_width;
        RECT {
            left: left.max(row.left),
            top: row.top - 6,
            right: right.max(row.left + panel_width),
            bottom: row.top + 108,
        }
    }

    fn address_pill_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let width = (rect.right - rect.left - 560).clamp(176, 258);
        let center = (rect.right + rect.left) / 2;
        let y = self.topbar_y();
        RECT {
            left: center - width / 2,
            top: y + 11,
            right: center + width / 2,
            bottom: y + 43,
        }
    }

    fn split_toolbar_rect(&self) -> Option<RECT> {
        self.active_split_group_id()?;
        if self.fullscreen {
            return None;
        }
        let address = self.address_pill_rect();
        let y = self.topbar_y();
        let right = self.extension_button_rect().left - 8;
        let left = (right - 88).max(address.right + 10);
        if right - left < 72 {
            return None;
        }
        Some(RECT {
            left,
            top: y + 14,
            right,
            bottom: y + 44,
        })
    }

    fn split_layout_button_rect(&self) -> Option<RECT> {
        let toolbar = self.split_toolbar_rect()?;
        Some(RECT {
            left: toolbar.left + 4,
            top: toolbar.top + 3,
            right: toolbar.left + 32,
            bottom: toolbar.bottom - 3,
        })
    }

    fn split_unsplit_button_rect(&self) -> Option<RECT> {
        let toolbar = self.split_toolbar_rect()?;
        Some(RECT {
            left: toolbar.right - 32,
            top: toolbar.top + 3,
            right: toolbar.right - 4,
            bottom: toolbar.bottom - 3,
        })
    }

    fn window_button_rects(&self) -> (RECT, RECT, RECT) {
        let rect = client_rect(self.hwnd);
        let y = self.topbar_y();
        (
            RECT {
                left: rect.right - 138,
                top: y,
                right: rect.right - 92,
                bottom: y + TOPBAR_HEIGHT,
            },
            RECT {
                left: rect.right - 92,
                top: y,
                right: rect.right - 46,
                bottom: y + TOPBAR_HEIGHT,
            },
            RECT {
                left: rect.right - 46,
                top: y,
                right: rect.right,
                bottom: y + TOPBAR_HEIGHT,
            },
        )
    }

    fn command_popup_rect(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let width = (rect.right - rect.left - 420).clamp(520, 800);
        let height = 304;
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
                    if rows
                        .iter()
                        .any(|row| normalize_url_for_dedup(&row.2) == norm_tab)
                    {
                        continue;
                    }
                    rows.push((Some(tab_index), tab.title.clone(), tab.url.clone()));
                }
            }
        }

        // 2. Get bookmarks matching query
        for bookmark in &self.bookmarks {
            let matches_query = query.is_empty()
                || bookmark.url.to_ascii_lowercase().contains(&query)
                || bookmark.title.to_ascii_lowercase().contains(&query)
                || bookmark
                    .tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&query));
            if !matches_query {
                continue;
            }
            let norm_bookmark = normalize_url_for_dedup(&bookmark.url);
            if rows
                .iter()
                .any(|row| normalize_url_for_dedup(&row.2) == norm_bookmark)
            {
                continue;
            }
            rows.push((
                None,
                format!("Bookmark: {}", bookmark.title),
                bookmark.url.clone(),
            ));
        }

        // 3. Get visited history sites matching query, sorted by frecency score descending
        let mut matched_history: Vec<&VisitedSite> = self
            .visited_sites
            .iter()
            .filter(|site| {
                if query.is_empty() {
                    true
                } else {
                    site.url.to_ascii_lowercase().contains(&query)
                        || site.title.to_ascii_lowercase().contains(&query)
                        || extract_search_query(&site.url)
                            .map(|q| q.to_ascii_lowercase().contains(&query))
                            .unwrap_or(false)
                }
            })
            .collect();

        matched_history.sort_by_cached_key(|site| {
            std::cmp::Reverse(calculate_frecency(
                site.visit_count,
                site.last_visit_time,
                now,
            ))
        });

        // Add history suggestions to rows
        for site in &matched_history {
            let norm_site = normalize_url_for_dedup(&site.url);
            if rows
                .iter()
                .any(|row| normalize_url_for_dedup(&row.2) == norm_site)
            {
                continue;
            }
            let title = if site.title.trim().is_empty() {
                label_for_url(&site.url)
            } else {
                site.title.clone()
            };
            rows.push((None, title, site.url.clone()));
        }

        // 4. If there is a query, sort the results to prioritize prefix matches
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
                if clean_all_prefixes(url)
                    .to_ascii_lowercase()
                    .starts_with(&query)
                {
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
            let display_url = if let Some(query) = extract_search_query(url) {
                if query
                    .to_ascii_lowercase()
                    .starts_with(&current_text.to_ascii_lowercase())
                {
                    Some(query)
                } else {
                    None
                }
            } else {
                let clean = clean_all_prefixes(url);
                if clean
                    .to_ascii_lowercase()
                    .starts_with(&current_text.to_ascii_lowercase())
                {
                    Some(clean.to_string())
                } else if url
                    .to_ascii_lowercase()
                    .starts_with(&current_text.to_ascii_lowercase())
                {
                    Some(url.to_string())
                } else {
                    None
                }
            };

            if let Some(display_url) = display_url {
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
    }

    fn new_tab_opacity(&self) -> f32 {
        1.0
    }

    fn web_content_bounds(&self) -> RECT {
        let rect = client_rect(self.hwnd);
        let sidebar_width = self.sidebar_width();
        let pushed_left = sidebar_width;
        if self.fullscreen {
            RECT {
                left: 0,
                top: 0,
                right: rect.right,
                bottom: rect.bottom,
            }
        } else {
            match self.sidebar_mode {
                SidebarMode::Hidden => {
                    if self.sidebar_target >= SIDEBAR_EXPANDED {
                        match self.sidebar_expand_mode {
                            SidebarMode::Overlay => RECT {
                                left: 0,
                                top: self.topbar_pushed_height(),
                                right: rect.right,
                                bottom: rect.bottom,
                            },
                            SidebarMode::Pushed => RECT {
                                left: pushed_left,
                                top: self.topbar_pushed_height(),
                                right: rect.right,
                                bottom: rect.bottom,
                            },
                            _ => RECT {
                                left: HOVER_ZONE,
                                top: self.topbar_pushed_height(),
                                right: rect.right,
                                bottom: rect.bottom,
                            },
                        }
                    } else {
                        RECT {
                            left: 0,
                            top: self.topbar_pushed_height(),
                            right: rect.right,
                            bottom: rect.bottom,
                        }
                    }
                }
                SidebarMode::Overlay => RECT {
                    left: 0,
                    top: self.topbar_pushed_height(),
                    right: rect.right,
                    bottom: rect.bottom,
                },
                SidebarMode::Pushed => RECT {
                    left: pushed_left,
                    top: self.topbar_pushed_height(),
                    right: rect.right,
                    bottom: rect.bottom,
                },
            }
        }
    }

    fn split_layout_rects_for(&self, bounds: RECT, layout: SplitLayout, count: usize) -> Vec<RECT> {
        let count = count.clamp(1, SPLIT_MAX_PANES);
        let width = (bounds.right - bounds.left).max(1);
        let height = (bounds.bottom - bounds.top).max(1);
        match layout {
            SplitLayout::Horizontal => {
                let total_gap = SPLIT_GAP * (count as i32 - 1).max(0);
                let pane_width = (width - total_gap).max(count as i32) / count as i32;
                (0..count)
                    .map(|i| {
                        let left = bounds.left + i as i32 * (pane_width + SPLIT_GAP);
                        let right = if i + 1 == count {
                            bounds.right
                        } else {
                            left + pane_width
                        };
                        RECT {
                            left,
                            top: bounds.top,
                            right,
                            bottom: bounds.bottom,
                        }
                    })
                    .collect()
            }
            SplitLayout::Vertical => {
                let total_gap = SPLIT_GAP * (count as i32 - 1).max(0);
                let pane_height = (height - total_gap).max(count as i32) / count as i32;
                (0..count)
                    .map(|i| {
                        let top = bounds.top + i as i32 * (pane_height + SPLIT_GAP);
                        let bottom = if i + 1 == count {
                            bounds.bottom
                        } else {
                            top + pane_height
                        };
                        RECT {
                            left: bounds.left,
                            top,
                            right: bounds.right,
                            bottom,
                        }
                    })
                    .collect()
            }
            SplitLayout::Grid => {
                let cols = match count {
                    1 => 1,
                    2 => 2,
                    _ => 2,
                };
                let rows = count.div_ceil(cols).max(1);
                let total_gap_x = SPLIT_GAP * (cols as i32 - 1).max(0);
                let total_gap_y = SPLIT_GAP * (rows as i32 - 1).max(0);
                let pane_width = (width - total_gap_x).max(cols as i32) / cols as i32;
                let pane_height = (height - total_gap_y).max(rows as i32) / rows as i32;
                (0..count)
                    .map(|i| {
                        let col = i % cols;
                        let row = i / cols;
                        let left = bounds.left + col as i32 * (pane_width + SPLIT_GAP);
                        let top = bounds.top + row as i32 * (pane_height + SPLIT_GAP);
                        RECT {
                            left,
                            top,
                            right: if col + 1 == cols {
                                bounds.right
                            } else {
                                left + pane_width
                            },
                            bottom: if row + 1 == rows {
                                bounds.bottom
                            } else {
                                top + pane_height
                            },
                        }
                    })
                    .collect()
            }
        }
    }

    fn active_pane_rects(&self, bounds: RECT) -> Vec<(usize, RECT)> {
        let Some(active_index) = self.active_tab_index() else {
            return Vec::new();
        };
        let Some(active_tab) = self.tabs.get(active_index) else {
            return Vec::new();
        };
        if let Some(group_id) = active_tab.split_group_id {
            if let Some(group) = self.split_group(group_id) {
                let rects =
                    self.split_layout_rects_for(bounds, group.layout, group.tab_ids.len());
                return group
                    .tab_ids
                    .iter()
                    .copied()
                    .zip(rects)
                    .collect();
            }
        }
        vec![(active_tab.id, bounds)]
    }

    fn split_preview_pane_rects(&self, bounds: RECT) -> Option<(Vec<(usize, RECT)>, RECT)> {
        let target = self.split_drop_target?;
        let drag = self.drag_state?;
        let DragSource::Tab(source_index) = drag.source else {
            return None;
        };
        let source_tab_id = self.tabs.get(source_index).map(|tab| tab.id)?;
        if source_tab_id == target.target_tab_id {
            return None;
        }
        let mut tab_ids = self
            .split_group_for_tab_id(target.target_tab_id)
            .and_then(|group_id| self.split_group(group_id))
            .map(|group| group.tab_ids.clone())
            .unwrap_or_else(|| vec![target.target_tab_id]);
        tab_ids.retain(|tab_id| *tab_id != source_tab_id);
        let target_pos = tab_ids
            .iter()
            .position(|tab_id| *tab_id == target.target_tab_id)?;
        if tab_ids.len() + 1 > SPLIT_MAX_PANES {
            return None;
        }
        let insert_at = match target.zone {
            SplitDropZone::Left | SplitDropZone::Top => target_pos,
            SplitDropZone::Right | SplitDropZone::Bottom => target_pos + 1,
        }
        .min(tab_ids.len());
        let mut slots: Vec<Option<usize>> = tab_ids.into_iter().map(Some).collect();
        slots.insert(insert_at, None);
        let layout = match target.zone {
            SplitDropZone::Left | SplitDropZone::Right => SplitLayout::Horizontal,
            SplitDropZone::Top | SplitDropZone::Bottom => SplitLayout::Vertical,
        };
        let rects = self.split_layout_rects_for(bounds, layout, slots.len());
        let mut existing = Vec::new();
        let mut placeholder = None;
        for (slot, rect) in slots.into_iter().zip(rects) {
            if let Some(tab_id) = slot {
                existing.push((tab_id, rect));
            } else {
                placeholder = Some(rect);
            }
        }
        placeholder.map(|placeholder| (existing, placeholder))
    }

    fn shrink_rect_for_split_preview(&self, rect: RECT, zone: SplitDropZone) -> RECT {
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        match zone {
            SplitDropZone::Left => RECT {
                left: rect.left + (width * 34 / 100).max(96),
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            },
            SplitDropZone::Right => RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right - (width * 34 / 100).max(96),
                bottom: rect.bottom,
            },
            SplitDropZone::Top => RECT {
                left: rect.left,
                top: rect.top + (height * 34 / 100).max(96),
                right: rect.right,
                bottom: rect.bottom,
            },
            SplitDropZone::Bottom => RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom - (height * 34 / 100).max(96),
            },
        }
    }

    fn split_placeholder_rect(&self) -> Option<RECT> {
        if let Some((_, placeholder)) = self.split_preview_pane_rects(self.web_content_bounds()) {
            return Some(placeholder);
        }
        let target = self.split_drop_target?;
        let bounds = self.web_content_bounds();
        let pane = self
            .active_pane_rects(bounds)
            .into_iter()
            .find(|(tab_id, _)| *tab_id == target.target_tab_id)
            .map(|(_, rect)| rect)?;
        let shrunken = self.shrink_rect_for_split_preview(pane, target.zone);
        Some(match target.zone {
            SplitDropZone::Left => RECT {
                left: pane.left,
                top: pane.top,
                right: shrunken.left - SPLIT_GAP,
                bottom: pane.bottom,
            },
            SplitDropZone::Right => RECT {
                left: shrunken.right + SPLIT_GAP,
                top: pane.top,
                right: pane.right,
                bottom: pane.bottom,
            },
            SplitDropZone::Top => RECT {
                left: pane.left,
                top: pane.top,
                right: pane.right,
                bottom: shrunken.top - SPLIT_GAP,
            },
            SplitDropZone::Bottom => RECT {
                left: pane.left,
                top: shrunken.bottom + SPLIT_GAP,
                right: pane.right,
                bottom: pane.bottom,
            },
        })
    }

    fn split_drop_zone_for_point(&self, rect: RECT, x: i32, y: i32) -> SplitDropZone {
        let left = (x - rect.left).abs();
        let right = (rect.right - x).abs();
        let top = (y - rect.top).abs();
        let bottom = (rect.bottom - y).abs();
        let mut best = (left, SplitDropZone::Left);
        if right < best.0 {
            best = (right, SplitDropZone::Right);
        }
        if top < best.0 {
            best = (top, SplitDropZone::Top);
        }
        if bottom < best.0 {
            best = (bottom, SplitDropZone::Bottom);
        }
        best.1
    }

    fn calculate_split_drop_target(&self, x: i32, y: i32) -> Option<SplitDropTarget> {
        let drag = self.drag_state?;
        if !drag.active {
            return None;
        }
        let DragSource::Tab(source_index) = drag.source else {
            return None;
        };
        let source_tab_id = self.tabs.get(source_index).map(|tab| tab.id)?;
        let bounds = self.web_content_bounds();
        if self.sidebar_width() > 92 && (x as f32) < self.sidebar_width {
            return None;
        }
        if self.topbar_height > 1.0 && y < self.topbar_height as i32 {
            return None;
        }
        if !point_in_rect(x, y, bounds) {
            return None;
        }
        for (target_tab_id, rect) in self.active_pane_rects(bounds) {
            if !point_in_rect(x, y, rect) || target_tab_id == source_tab_id {
                continue;
            }
            let target_group_len = self
                .split_group_for_tab_id(target_tab_id)
                .and_then(|group_id| self.split_group(group_id))
                .map(|group| group.tab_ids.len())
                .unwrap_or(1);
            let same_group = self.split_group_for_tab_id(source_tab_id)
                == self.split_group_for_tab_id(target_tab_id);
            if target_group_len >= SPLIT_MAX_PANES && !same_group {
                return None;
            }
            return Some(SplitDropTarget {
                target_tab_id,
                zone: self.split_drop_zone_for_point(rect, x, y),
            });
        }
        None
    }

    fn layout(&self) {
        let rect = client_rect(self.hwnd);
        unsafe {
            let flags = WindowsAndMessaging::SWP_NOZORDER;
            if self.command_open && !self.fullscreen {
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
            if self.find_open && !self.fullscreen {
                let input = self.find_input_rect();
                let _ = WindowsAndMessaging::SetWindowPos(
                    self.find_hwnd,
                    Some(HWND_TOP),
                    input.left,
                    input.top,
                    (input.right - input.left).max(80),
                    input.bottom - input.top,
                    flags,
                );
                let _ =
                    WindowsAndMessaging::ShowWindow(self.find_hwnd, WindowsAndMessaging::SW_SHOW);
            } else {
                let _ =
                    WindowsAndMessaging::ShowWindow(self.find_hwnd, WindowsAndMessaging::SW_HIDE);
            }
        }

        let sidebar_width = self.sidebar_width();
        let bounds = self.web_content_bounds();
        let last = self.last_bounds_rect.get();
        let size_changed = bounds.left != last.left
            || bounds.right != last.right
            || bounds.top != last.top
            || bounds.bottom != last.bottom;

        let needs_clipping = !self.fullscreen
            && (self.sidebar_mode == SidebarMode::Overlay
                || (self.sidebar_mode == SidebarMode::Hidden
                    && self.sidebar_expand_mode == SidebarMode::Overlay
                    && self.sidebar_target >= SIDEBAR_EXPANDED)
                || self.topbar_mode == SidebarMode::Overlay
                || (self.topbar_mode == SidebarMode::Hidden
                    && self.topbar_expand_mode == SidebarMode::Overlay
                    && self.topbar_target >= TOPBAR_EXPANDED));
        let clip_changed = needs_clipping
            && (sidebar_width > 0 || self.topbar_height > 0.0)
            && ((sidebar_width as f32 - self.last_clip_width.get()).abs() > 1.0
                || (self.topbar_height - self.last_clip_top.get()).abs() > 1.0
                || size_changed);
        let was_clipped = self.last_clip_width.get() != 0.0 || self.last_clip_top.get() != 0.0;
        let should_clear =
            (!needs_clipping || (sidebar_width <= 0 && self.topbar_height <= 0.0)) && was_clipped;
        let active_panes = self
            .split_preview_pane_rects(bounds)
            .map(|(panes, _)| panes)
            .unwrap_or_else(|| self.active_pane_rects(bounds));
        for tab in self.tabs.iter() {
            unsafe {
                let pane_rect = active_panes
                    .iter()
                    .find(|(tab_id, _)| *tab_id == tab.id)
                    .map(|(_, rect)| *rect);
                let is_visible = pane_rect.is_some()
                    && tab.workspace_id == self.active_workspace
                    && !tab.unloaded;
                if let Some(tab_bounds) = pane_rect {
                    let _ = tab.controller.SetBounds(tab_bounds);
                    let _ = WindowsAndMessaging::SetWindowPos(
                        tab.child_hwnd,
                        None,
                        tab_bounds.left,
                        tab_bounds.top,
                        tab_bounds.right - tab_bounds.left,
                        tab_bounds.bottom - tab_bounds.top,
                        WindowsAndMessaging::SWP_NOZORDER | WindowsAndMessaging::SWP_NOACTIVATE,
                    );
                }
                if clip_changed {
                    let clip_left = sidebar_width;
                    let clip_top = if self.topbar_mode == SidebarMode::Overlay
                        || self.topbar_expand_mode == SidebarMode::Overlay
                    {
                        self.topbar_height as i32
                    } else {
                        0
                    };
                    let clip_right = rect.right;
                    let clip_bottom = rect.bottom - self.topbar_pushed_height();
                    let region = CreateRectRgn(clip_left, clip_top, clip_right, clip_bottom);
                    let _ = SetWindowRgn(tab.child_hwnd, Some(region), true);
                } else if should_clear {
                    let _ = SetWindowRgn(tab.child_hwnd, None, true);
                }
                let _ = tab.controller.SetIsVisible(is_visible);
            }
        }
        unsafe {
            if self.download_popup_hwnd != HWND(std::ptr::null_mut()) && self.sidebar_width < 1.0 {
                if let Some(toast) = &self.download_toast {
                    let _ = WindowsAndMessaging::SetWindowPos(
                        self.download_popup_hwnd,
                        Some(HWND_TOP),
                        (116.0 + toast.slide_x) as i32,
                        rect.bottom - 52,
                        32,
                        32,
                        WindowsAndMessaging::SWP_NOACTIVATE,
                    );
                }
            }
        }
        if clip_changed {
            self.last_clip_width.set(sidebar_width as f32);
            self.last_clip_top.set(self.topbar_height);
        } else if should_clear {
            self.last_clip_width.set(0.0);
            self.last_clip_top.set(0.0);
        }
        if size_changed {
            self.last_bounds_rect.set(bounds);
        }
        self.position_rename_edit();
    }

    fn paint(&self, hdc: HDC) {
        let rect = client_rect(self.hwnd);
        unsafe {
            let _ = FillRect(hdc, &rect, self.brushes.black);
        }
        if self.fullscreen {
            let is_unloaded = self
                .tabs
                .get(self.active)
                .map(|t| t.unloaded)
                .unwrap_or(false);
            if self.active_tab_index().is_none() || is_unloaded {
                self.paint_cached_background(hdc, rect);
            }
            return;
        }
        let sidebar_width = self.sidebar_width();
        let is_overlay = self.sidebar_mode == SidebarMode::Overlay;
        unsafe {
            let topbar = RECT {
                left: 0,
                top: self.topbar_y(),
                right: rect.right,
                bottom: self.topbar_y() + TOPBAR_HEIGHT,
            };
            let _ = FillRect(hdc, &topbar, self.brushes.secondary);
            fill_rect(
                hdc,
                RECT {
                    left: 0,
                    top: self.topbar_y() + TOPBAR_HEIGHT - 1,
                    right: rect.right,
                    bottom: self.topbar_y() + TOPBAR_HEIGHT,
                },
                0x202020,
            );

            let is_unloaded = self
                .tabs
                .get(self.active)
                .map(|t| t.unloaded)
                .unwrap_or(false);
            if self.active_tab_index().is_none() || is_unloaded {
                self.paint_cached_background(
                    hdc,
                    RECT {
                        left: 0,
                        top: self.topbar_pushed_height(),
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                );
            }

            if sidebar_width >= 1 {
                let sidebar = RECT {
                    left: 0,
                    top: self.topbar_pushed_height(),
                    right: sidebar_width,
                    bottom: rect.bottom,
                };
                let _ = FillRect(hdc, &sidebar, self.brushes.secondary);
                if !is_overlay {
                    fill_rect(
                        hdc,
                        RECT {
                            left: sidebar.right - 1,
                            top: self.topbar_pushed_height(),
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
                &self.fonts.nav_icon,
            );
            draw_toolbar_icon_button(
                hdc,
                forward,
                IconKind::Forward,
                self.hover_target == Some(HoverTarget::Forward),
                &self.fonts.nav_icon,
            );
            draw_toolbar_icon_button(
                hdc,
                reload,
                IconKind::Reload,
                self.hover_target == Some(HoverTarget::Reload),
                &self.fonts.toolbar_icon,
            );
            draw_toolbar_icon_button(
                hdc,
                self.extension_button_rect(),
                IconKind::Extensions,
                self.hover_target == Some(HoverTarget::ExtensionsButton),
                &self.fonts.toolbar_icon,
            );

            let edit_rect = self.address_pill_rect();
            let active_is_loading = self
                .active_tab_index()
                .and_then(|idx| self.tabs.get(idx))
                .map(|t| t.is_loading)
                .unwrap_or(false);
            if self.hover_target == Some(HoverTarget::Address)
                || self.command_open
                || active_is_loading
            {
                let full_left = edit_rect.left + 22;
                let full_right = edit_rect.right - 22;

                if active_is_loading {
                    let time_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let width = full_right - full_left;
                    if width > 0 {
                        let block_width = (width as f64 * 0.7) as i32;
                        let cycle_duration: u128 = 1200;
                        let t = (time_ms % cycle_duration) as f64 / cycle_duration as f64;
                        // Smooth ease-in-out: accelerate from left, decelerate to right
                        let eased = t * t * (3.0 - 2.0 * t);
                        // Sweep from off-screen left to off-screen right
                        let start = full_left - block_width;
                        let end = full_right;
                        let total_travel = end - start;
                        let anim_left = start + (eased * total_travel as f64) as i32;
                        let anim_right = anim_left + block_width;

                        let fl = anim_left.max(full_left);
                        let fr = anim_right.min(full_right);

                        if fl < fr {
                            fill_rect(
                                hdc,
                                RECT {
                                    left: fl,
                                    top: edit_rect.bottom - 2,
                                    right: fr,
                                    bottom: edit_rect.bottom - 1,
                                },
                                self.accent_color,
                            );
                        }
                    }
                } else {
                    fill_rect(
                        hdc,
                        RECT {
                            left: full_left,
                            top: edit_rect.bottom - 2,
                            right: full_right,
                            bottom: edit_rect.bottom - 1,
                        },
                        self.accent_color,
                    );
                }
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
                    right: edit_rect.right - 44,
                    bottom: edit_rect.bottom,
                },
                COLOR_TEXT,
            );
            if self.hover_target == Some(HoverTarget::Address)
                || self.hover_target == Some(HoverTarget::AddressMenu)
                || matches!(
                    self.overlay_menu.as_ref().map(|menu| menu.target),
                    Some(MenuTarget::AddressMenu)
                )
            {
                let menu_rect = self.address_menu_rect();
                if self.hover_target == Some(HoverTarget::AddressMenu) {
                    fill_round_rect(hdc, menu_rect, COLOR_SURFACE_HOVER, 8);
                }
                draw_centered_text(hdc, &self.fonts.body, "...", menu_rect, COLOR_MUTED);
            }

            draw_settings_button(
                hdc,
                self.settings_rect(),
                self.hover_target == Some(HoverTarget::Settings),
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
                fill_rect(
                    hdc,
                    RECT {
                        left: cx - 6,
                        top: cy,
                        right: cx + 6,
                        bottom: cy + 1,
                    },
                    COLOR_TEXT,
                );
            }

            // Draw Maximize Button
            let max_hover = self.hover_target == Some(HoverTarget::MaxButton);
            if max_hover {
                let _ = FillRect(hdc, &max_btn, self.brushes.hover);
            }
            {
                let cx = (max_btn.left + max_btn.right) / 2;
                let cy = (max_btn.top + max_btn.bottom) / 2;
                fill_rect(
                    hdc,
                    RECT {
                        left: cx - 5,
                        top: cy - 5,
                        right: cx + 5,
                        bottom: cy - 4,
                    },
                    COLOR_TEXT,
                );
                fill_rect(
                    hdc,
                    RECT {
                        left: cx - 5,
                        top: cy + 4,
                        right: cx + 5,
                        bottom: cy + 5,
                    },
                    COLOR_TEXT,
                );
                fill_rect(
                    hdc,
                    RECT {
                        left: cx - 5,
                        top: cy - 4,
                        right: cx - 4,
                        bottom: cy + 4,
                    },
                    COLOR_TEXT,
                );
                fill_rect(
                    hdc,
                    RECT {
                        left: cx + 4,
                        top: cy - 4,
                        right: cx + 5,
                        bottom: cy + 4,
                    },
                    COLOR_TEXT,
                );
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
                    fill_rect(
                        hdc,
                        RECT {
                            left: cx + i,
                            top: cy + i,
                            right: cx + i + 1,
                            bottom: cy + i + 1,
                        },
                        color,
                    );
                    fill_rect(
                        hdc,
                        RECT {
                            left: cx + i,
                            top: cy - i,
                            right: cx + i + 1,
                            bottom: cy - i + 1,
                        },
                        color,
                    );
                }
            }

            self.paint_split_toolbar(hdc);
            self.paint_split_drop_overlay(hdc);

            if sidebar_width > 92 {
                self.paint_workspace_header(hdc);
                let has_pinned = self
                    .folders
                    .iter()
                    .any(|f| f.workspace_id == self.active_workspace && f.pinned)
                    || self
                        .tabs
                        .iter()
                        .any(|t| t.workspace_id == self.active_workspace && t.pinned);
                if !has_pinned {
                    if let Some(rect) = self.pinned_section_rect() {
                        if let Ok(large_pin_font) =
                            create_font_with_face(58, 400, w!("Segoe Fluent Icons"))
                        {
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
                                self.accent_color,
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
                        SidebarRow::SplitGroup(group_id) => {
                            self.paint_split_group_row(hdc, group_id, row_rect)
                        }
                        SidebarRow::Tab(index) => {
                            if let Some(tab) = self.tabs.get(index) {
                                self.paint_tab(hdc, index, tab, row_rect, false);
                            }
                        }
                        SidebarRow::TabGhost(index) => {
                            if let Some(tab) = self.tabs.get(index) {
                                self.paint_tab(hdc, index, tab, row_rect, true);
                            }
                        }
                    }
                }
                self.paint_drop_target_highlight(hdc);
                self.paint_workspace_switcher(hdc);
                self.paint_download_indicators(hdc);
            }

            if self.find_open {
                self.paint_find_bar(hdc);
            }
            if self.download_panel.is_some() {
                if self.download_panel_reveal >= 0.995 {
                    self.paint_download_panel(hdc);
                } else if self.download_panel_reveal > 0.005 {
                    if let Some(panel) = self.download_panel_rect() {
                        let pw = panel.right - panel.left;
                        let ph = panel.bottom - panel.top;
                        if pw > 0 && ph > 0 {
                            let mem_dc = {
                                let mut cache = self.dl_panel_cache.borrow_mut();
                                let cached = cache.get_or_insert_with(|| {
                                    let dc = CreateCompatibleDC(Some(hdc));
                                    let bitmap = CreateCompatibleBitmap(hdc, pw, ph);
                                    let old = SelectObject(dc, HGDIOBJ(bitmap.0));
                                    PaintCache {
                                        bitmap,
                                        dc,
                                        width: pw,
                                        height: ph,
                                        old_bitmap: old,
                                    }
                                });
                                if cached.width != pw || cached.height != ph {
                                    let _ = SelectObject(cached.dc, cached.old_bitmap);
                                    let _ = DeleteObject(HGDIOBJ(cached.bitmap.0));
                                    let _ = DeleteDC(cached.dc);
                                    let dc = CreateCompatibleDC(Some(hdc));
                                    let bitmap = CreateCompatibleBitmap(hdc, pw, ph);
                                    let old = SelectObject(dc, HGDIOBJ(bitmap.0));
                                    *cached = PaintCache {
                                        bitmap,
                                        dc,
                                        width: pw,
                                        height: ph,
                                        old_bitmap: old,
                                    };
                                }
                                cached.dc
                            };
                            if !mem_dc.is_invalid() {
                                fill_rect(
                                    mem_dc,
                                    RECT {
                                        left: panel.left,
                                        top: panel.top,
                                        right: panel.right,
                                        bottom: panel.bottom,
                                    },
                                    0x151515,
                                );
                                let _ = SetViewportOrgEx(mem_dc, -panel.left, -panel.top, None);
                                self.paint_download_panel(mem_dc);
                                let _ = SetViewportOrgEx(mem_dc, 0, 0, None);
                                let alpha = (self.download_panel_reveal * 255.0) as u8;
                                let blend = BLENDFUNCTION {
                                    BlendOp: AC_SRC_OVER as u8,
                                    BlendFlags: 0,
                                    SourceConstantAlpha: alpha,
                                    AlphaFormat: 0,
                                };
                                let _ = AlphaBlend(
                                    hdc, panel.left, panel.top, pw, ph, mem_dc, 0, 0, pw, ph, blend,
                                );
                            }
                        }
                    }
                }
            }

            if sidebar_width <= 92 {
                self.paint_download_toast(hdc, rect);
            }

            if self.show_default_bubble {
                let width_f = sidebar_width as f32;
                let alpha = (((width_f - 120.0) / 120.0).clamp(0.0, 1.0) * 255.0) as u8;
                if alpha > 0 {
                    self.paint_default_bubble(hdc, alpha);
                }
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

    fn default_bubble_rect(&self) -> Option<RECT> {
        if !self.show_default_bubble {
            return None;
        }
        let rect = client_rect(self.hwnd);
        let left = 12;
        let right = left + 224;
        Some(RECT {
            left,
            top: rect.bottom - 208,
            right,
            bottom: rect.bottom - 104,
        })
    }

    fn default_bubble_close_rect(&self) -> Option<RECT> {
        let bubble = self.default_bubble_rect()?;
        Some(RECT {
            left: bubble.right - 28,
            top: bubble.top + 8,
            right: bubble.right - 8,
            bottom: bubble.top + 28,
        })
    }

    fn default_bubble_button_rect(&self) -> Option<RECT> {
        let bubble = self.default_bubble_rect()?;
        Some(RECT {
            left: bubble.left + 12,
            top: bubble.top + 54,
            right: bubble.right - 12,
            bottom: bubble.top + 92,
        })
    }

    fn paint_default_bubble(&self, hdc: HDC, alpha: u8) {
        let Some(rect) = self.default_bubble_rect() else {
            return;
        };
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        unsafe {
            // Create a memory DC compatible with hdc to draw the entire bubble offscreen
            let mem_dc = CreateCompatibleDC(Some(hdc));
            if !mem_dc.is_invalid() {
                let bitmap = CreateCompatibleBitmap(hdc, width, height);
                if !bitmap.is_invalid() {
                    let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

                    // Use viewport origin shift so all existing coordinate variables can remain unchanged
                    let _ = SetViewportOrgEx(mem_dc, -rect.left, -rect.top, None);

                    // 1. Draw rounded slate-grey background onto mem_dc (clipped to round region in local coordinates)
                    let rgn = CreateRoundRectRgn(
                        rect.left,
                        rect.top,
                        rect.right + 1,
                        rect.bottom + 1,
                        16,
                        16,
                    );
                    let _ = SelectClipRgn(mem_dc, Some(rgn));
                    fill_rect(
                        mem_dc,
                        RECT {
                            left: rect.left,
                            top: rect.top,
                            right: rect.right,
                            bottom: rect.bottom,
                        },
                        self.secondary_color,
                    );
                    let _ = SelectClipRgn(mem_dc, None);
                    let _ = DeleteObject(HGDIOBJ(rgn.0));

                    // 2. Draw outline around the bubble with accent color
                    draw_outline(mem_dc, rect, self.accent_color, 16);

                    // 3. Draw close button
                    let close_rect = self.default_bubble_close_rect().unwrap();
                    let is_close_hovered =
                        self.hover_target == Some(HoverTarget::DefaultBubbleClose);
                    if is_close_hovered {
                        fill_round_rect(mem_dc, close_rect, COLOR_SURFACE_HOVER, 6);
                    }
                    draw_icon_glyph(
                        mem_dc,
                        &self.fonts.toolbar_icon,
                        "\u{E8BB}",
                        close_rect,
                        if is_close_hovered {
                            0xffffff
                        } else {
                            COLOR_MUTED
                        },
                    );

                    // 4. Draw messages
                    let line1_rect = RECT {
                        left: rect.left + 14,
                        top: rect.top + 10,
                        right: rect.right - 36,
                        bottom: rect.top + 28,
                    };
                    draw_text(
                        mem_dc,
                        &self.fonts.body,
                        "Make Aster default browser?",
                        line1_rect,
                        COLOR_TEXT,
                    );

                    let line2_rect = RECT {
                        left: rect.left + 14,
                        top: rect.top + 28,
                        right: rect.right - 36,
                        bottom: rect.top + 46,
                    };
                    draw_text(
                        mem_dc,
                        &self.fonts.small,
                        "For a faster web experience.",
                        line2_rect,
                        COLOR_MUTED,
                    );

                    // 5. Draw primary action button
                    let btn_rect = self.default_bubble_button_rect().unwrap();
                    let is_btn_hovered =
                        self.hover_target == Some(HoverTarget::DefaultBubbleSetDefault);
                    let btn_bg = if is_btn_hovered {
                        self.accent_color
                    } else {
                        0x343434
                    };
                    let btn_fg = if is_btn_hovered { 0x000000 } else { 0xffffff };
                    fill_round_rect(mem_dc, btn_rect, btn_bg, 8);
                    if !is_btn_hovered {
                        draw_outline(mem_dc, btn_rect, 0x454545, 8);
                    }
                    draw_centered_text(
                        mem_dc,
                        &self.fonts.body,
                        "Set as Default",
                        btn_rect,
                        btn_fg,
                    );

                    // Reset viewport origin before alpha blending onto the destination
                    let _ = SetViewportOrgEx(mem_dc, 0, 0, None);

                    // Select a rounded clip region on the destination hdc to cleanly truncate corners
                    let dest_rgn = CreateRoundRectRgn(
                        rect.left,
                        rect.top,
                        rect.right + 1,
                        rect.bottom + 1,
                        16,
                        16,
                    );
                    let _ = SelectClipRgn(hdc, Some(dest_rgn));

                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: alpha,
                        AlphaFormat: 0,
                    };

                    let _ = AlphaBlend(
                        hdc, rect.left, rect.top, width, height, mem_dc, 0, 0, width, height, blend,
                    );

                    // Restore clipping region on hdc and clean up resources
                    let _ = SelectClipRgn(hdc, None);
                    let _ = DeleteObject(HGDIOBJ(dest_rgn.0));

                    let _ = SelectObject(mem_dc, old_bitmap);
                    let _ = DeleteObject(HGDIOBJ(bitmap.0));
                }
                let _ = DeleteDC(mem_dc);
            }
        }
    }

    fn paint_overlay_menu(&self, hdc: HDC, menu: &OverlayMenu) {
        unsafe {
            let width = menu.rect.right - menu.rect.left;
            let height = menu.rect.bottom - menu.rect.top;
            let local_rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
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
            fill_round_rect(hdc, menu, self.secondary_color, 12);
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
                "Site theme",
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

            let store_row = self.extension_store_row_rect();
            if self.hover_target == Some(HoverTarget::ExtensionStore) {
                fill_round_rect(hdc, store_row, COLOR_SURFACE_HOVER, 9);
            }
            draw_text(
                hdc,
                &self.fonts.small,
                "Extension Store",
                RECT {
                    left: store_row.left + 12,
                    top: store_row.top,
                    right: store_row.right - 30,
                    bottom: store_row.bottom,
                },
                COLOR_TEXT,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xE719).as_str(),
                RECT {
                    left: store_row.right - 28,
                    top: store_row.top,
                    right: store_row.right - 6,
                    bottom: store_row.bottom,
                },
                COLOR_MUTED,
            );

            let extensions_row = self.extensions_page_row_rect();
            if self.hover_target == Some(HoverTarget::ExtensionsPage) {
                fill_round_rect(hdc, extensions_row, COLOR_SURFACE_HOVER, 9);
            }
            draw_text(
                hdc,
                &self.fonts.small,
                "Extensions",
                RECT {
                    left: extensions_row.left + 12,
                    top: extensions_row.top,
                    right: extensions_row.right - 30,
                    bottom: extensions_row.bottom,
                },
                COLOR_TEXT,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xECAA).as_str(),
                RECT {
                    left: extensions_row.right - 28,
                    top: extensions_row.top,
                    right: extensions_row.right - 6,
                    bottom: extensions_row.bottom,
                },
                COLOR_MUTED,
            );

            let history_row = self.history_page_row_rect();
            if self.hover_target == Some(HoverTarget::HistoryPage) {
                fill_round_rect(hdc, history_row, COLOR_SURFACE_HOVER, 9);
            }
            draw_text(
                hdc,
                &self.fonts.small,
                "History",
                RECT {
                    left: history_row.left + 12,
                    top: history_row.top,
                    right: history_row.right - 30,
                    bottom: history_row.bottom,
                },
                COLOR_TEXT,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xE81C).as_str(),
                RECT {
                    left: history_row.right - 28,
                    top: history_row.top,
                    right: history_row.right - 6,
                    bottom: history_row.bottom,
                },
                COLOR_MUTED,
            );

            let settings_row = self.settings_page_row_rect();
            if self.hover_target == Some(HoverTarget::SettingsPage) {
                fill_round_rect(hdc, settings_row, COLOR_SURFACE_HOVER, 9);
            }
            draw_text(
                hdc,
                &self.fonts.small,
                "Settings",
                RECT {
                    left: settings_row.left + 12,
                    top: settings_row.top,
                    right: settings_row.right - 30,
                    bottom: settings_row.bottom,
                },
                COLOR_TEXT,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xE713).as_str(),
                RECT {
                    left: settings_row.right - 28,
                    top: settings_row.top,
                    right: settings_row.right - 6,
                    bottom: settings_row.bottom,
                },
                COLOR_MUTED,
            );

            if self.mode_menu_open {
                let options = self.mode_options_rect();
                fill_round_rect(hdc, options, self.secondary_color, 12);
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
                            self.accent_color,
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

    fn paint_download_indicators(&self, hdc: HDC) {
        if self.download_collapse_anim.is_some() {
            self.paint_download_collapse(hdc);
            return;
        }
        if self.download_removal_anim.is_some() {
            self.paint_download_indicators_animating(hdc);
            return;
        }
        let rects = self.download_indicator_rects();
        if rects.is_empty() {
            return;
        }
        unsafe {
            for (target, rect) in rects {
                match target {
                    Some(id) => {
                        if let Some(download) = self.downloads.iter().find(|item| item.id == id) {
                            draw_download_indicator(
                                hdc,
                                rect,
                                self.download_progress(download),
                                download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                                download.completed_at,
                                download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                                download.cancelled_at,
                                self.hover_target == Some(HoverTarget::DownloadIndicator(id)),
                            );
                        }
                    }
                    None => {
                        let extra = self.downloads.len().saturating_sub(3);
                        self.paint_download_overflow(hdc, rect, extra);
                    }
                }
            }
        }
    }

    fn paint_download_indicators_animating(&self, hdc: HDC) {
        let anim = match &self.download_removal_anim {
            Some(a) => a,
            None => return,
        };
        let elapsed = anim.start_time.elapsed().as_millis();
        let progress = (elapsed as f32 / anim.duration as f32).min(1.0);
        let settings = self.settings_rect();
        let start_x = settings.right + 14;
        let y = settings.top;

        unsafe {
            if anim.old_count > 3 && anim.old_count == 4 {
                let overflow_cx = start_x + 20;
                let _cy = y + 16;

                let ease = 1.0 - (1.0 - progress) * (1.0 - progress) * (1.0 - progress);
                for (ni, download) in self.downloads.iter().enumerate() {
                    let target_x = start_x + ni as i32 * 40;
                    let cur_x =
                        overflow_cx - 16 + ((target_x + 16 - overflow_cx) as f32 * ease) as i32;
                    let rect = RECT {
                        left: cur_x,
                        top: y,
                        right: cur_x + 32,
                        bottom: y + 32,
                    };
                    draw_download_indicator(
                        hdc,
                        rect,
                        self.download_progress(download),
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                        download.completed_at,
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                        download.cancelled_at,
                        self.hover_target == Some(HoverTarget::DownloadIndicator(download.id)),
                    );
                }
            } else if anim.removed_index == anim.old_count - 1 {
                for (ni, download) in self.downloads.iter().enumerate() {
                    let rect = RECT {
                        left: start_x + ni as i32 * 40,
                        top: y,
                        right: start_x + ni as i32 * 40 + 32,
                        bottom: y + 32,
                    };
                    draw_download_indicator(
                        hdc,
                        rect,
                        self.download_progress(download),
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                        download.completed_at,
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                        download.cancelled_at,
                        self.hover_target == Some(HoverTarget::DownloadIndicator(download.id)),
                    );
                }
                let fade_alpha = 1.0 - progress;
                if fade_alpha > 0.02 {
                    let old_rect = RECT {
                        left: start_x + anim.removed_index as i32 * 40,
                        top: y,
                        right: start_x + anim.removed_index as i32 * 40 + 32,
                        bottom: y + 32,
                    };
                    self.paint_download_indicator_faded(hdc, old_rect, fade_alpha, anim);
                }
            } else if anim.old_count <= 3 {
                for (ni, download) in self.downloads.iter().enumerate() {
                    let old_slot = if ni < anim.removed_index { ni } else { ni + 1 };
                    let old_x = start_x + old_slot as i32 * 40;
                    let new_x = start_x + ni as i32 * 40;
                    let ease = 1.0 - (1.0 - progress) * (1.0 - progress);
                    let cur_x = old_x + ((new_x - old_x) as f32 * ease) as i32;
                    let rect = RECT {
                        left: cur_x,
                        top: y,
                        right: cur_x + 32,
                        bottom: y + 32,
                    };
                    draw_download_indicator(
                        hdc,
                        rect,
                        self.download_progress(download),
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                        download.completed_at,
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                        download.cancelled_at,
                        self.hover_target == Some(HoverTarget::DownloadIndicator(download.id)),
                    );
                }
            }
        }
    }

    fn paint_download_collapse(&self, hdc: HDC) {
        let anim = match &self.download_collapse_anim {
            Some(a) => a,
            None => return,
        };
        let elapsed = anim.start_time.elapsed().as_millis();
        let progress = (elapsed as f32 / anim.duration as f32).min(1.0);
        let ease = 1.0 - (1.0 - progress) * (1.0 - progress);
        let settings = self.settings_rect();
        let start_x = settings.right + 14;
        let y = settings.top;
        unsafe {
            for i in 0..self.downloads.len().min(3) {
                if let Some(download) = self.downloads.get(i) {
                    let start_xi = start_x + i as i32 * 40;
                    let end_offset = match i {
                        0 => 10,
                        1 => 5,
                        _ => 0,
                    };
                    let end_xi = start_x + end_offset;
                    let cur_x = start_xi + ((end_xi - start_xi) as f32 * ease) as i32;
                    let rect = RECT {
                        left: cur_x,
                        top: y,
                        right: cur_x + 32,
                        bottom: y + 32,
                    };
                    draw_download_indicator(
                        hdc,
                        rect,
                        self.download_progress(download),
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                        download.completed_at,
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                        download.cancelled_at,
                        false,
                    );
                }
            }
        }
    }

    unsafe fn paint_download_indicator_faded(
        &self,
        hdc: HDC,
        rect: RECT,
        fade_alpha: f32,
        anim: &DownloadRemovalAnim,
    ) {
        let size = rect.right - rect.left;
        let mem_dc = CreateCompatibleDC(Some(hdc));
        let bitmap = CreateCompatibleBitmap(hdc, size, size);
        let old = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        fill_rect(
            mem_dc,
            RECT {
                left: 0,
                top: 0,
                right: size,
                bottom: size,
            },
            COLOR_PANEL_2,
        );
        draw_download_indicator(
            mem_dc,
            RECT {
                left: 0,
                top: 0,
                right: size,
                bottom: size,
            },
            anim.removed_progress,
            anim.removed_completed,
            anim.removed_completed_at,
            anim.removed_cancelled,
            anim.removed_cancelled_at,
            self.hover_target == Some(HoverTarget::DownloadIndicator(anim.removed_id)),
        );
        let src_alpha = (fade_alpha * 255.0) as u8;
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: src_alpha,
            AlphaFormat: 0,
        };
        let _ = AlphaBlend(
            hdc, rect.left, rect.top, size, size, mem_dc, 0, 0, size, size, blend,
        );
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
    }

    fn paint_download_toast(&self, hdc: HDC, window_rect: RECT) {
        let Some(toast) = &self.download_toast else {
            return;
        };
        if self.sidebar_width() > 92 {
            return;
        }
        let elapsed = toast.start_time.elapsed().as_millis();
        if elapsed >= 3000 && !toast.fading {
            return;
        }
        let rect = RECT {
            left: 62,
            top: window_rect.bottom - 52,
            right: 94,
            bottom: window_rect.bottom - 20,
        };
        if self.sidebar_width() < 1 {
            return;
        }
        let alpha = if toast.fading {
            let fade_progress = (self.sidebar_width / SIDEBAR_EXPANDED).clamp(0.0, 1.0);
            (1.0 - fade_progress).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if alpha <= 0.02 {
            return;
        }
        unsafe {
            draw_download_toast_gdi(hdc, rect, elapsed as u64, alpha);
        }
    }

    fn paint_download_overflow(&self, hdc: HDC, rect: RECT, extra: usize) {
        unsafe {
            if self.hover_target == Some(HoverTarget::DownloadOverflow) {
                fill_round_rect(hdc, rect, COLOR_SURFACE_HOVER, 16);
            }
            for (i, offset) in [10, 5, 0].iter().enumerate() {
                if let Some(download) = self.downloads.get(i) {
                    let circle = RECT {
                        left: rect.left + offset,
                        top: rect.top,
                        right: rect.left + offset + 32,
                        bottom: rect.bottom,
                    };
                    draw_download_indicator(
                        hdc,
                        circle,
                        self.download_progress(download),
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
                        download.completed_at,
                        download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
                        download.cancelled_at,
                        self.hover_target == Some(HoverTarget::DownloadOverflow),
                    );
                }
            }
            draw_text(
                hdc,
                &self.fonts.body,
                &format!("+{}", extra),
                RECT {
                    left: rect.left + 40,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                },
                COLOR_TEXT,
            );
        }
    }

    fn paint_download_panel(&self, hdc: HDC) {
        let Some(panel) = self.download_panel_rect() else {
            return;
        };
        unsafe {
            fill_round_rect(hdc, panel, self.secondary_color, 12);
            draw_outline(hdc, panel, COLOR_BORDER, 12);
            let rows = self.download_panel_rows();
            let mut top = panel.top + 9;
            for (index, download) in rows.iter().enumerate() {
                let row = RECT {
                    left: panel.left + 12,
                    top,
                    right: panel.right - 12,
                    bottom: top + 50,
                };
                let cancel = RECT {
                    left: row.right - 22,
                    top: row.top + 4,
                    right: row.right,
                    bottom: row.top + 26,
                };
                let open = RECT {
                    left: row.right - 50,
                    top: row.top + 4,
                    right: row.right - 28,
                    bottom: row.top + 26,
                };
                let show_pause = download.state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
                let pause = RECT {
                    left: row.right - 78,
                    top: row.top + 4,
                    right: row.right - 56,
                    bottom: row.top + 26,
                };

                draw_text(
                    hdc,
                    &self.fonts.body,
                    &download.file_name,
                    RECT {
                        left: row.left + 2,
                        top: row.top,
                        right: if show_pause {
                            pause.left - 8
                        } else {
                            open.left - 8
                        },
                        bottom: row.top + 24,
                    },
                    COLOR_TEXT,
                );

                let state_label = download_state_label(download);
                let size_label = if download.total_bytes > 0 {
                    let (recv_val, recv_unit) = format_bytes_split(download.received_bytes);
                    let (total_val, total_unit) = format_bytes_split(download.total_bytes);
                    if recv_unit == total_unit {
                        format!("{}/{}{}", recv_val, total_val, recv_unit)
                    } else {
                        format!("{}{}/{}{}", recv_val, recv_unit, total_val, total_unit)
                    }
                } else {
                    let (val, unit) = format_bytes_split(download.received_bytes);
                    format!("{}{}", val, unit)
                };
                draw_text(
                    hdc,
                    &self.fonts.small,
                    &format!("{}  {}", size_label, state_label),
                    RECT {
                        left: row.left + 2,
                        top: row.top + 22,
                        right: row.right - 2,
                        bottom: row.top + 42,
                    },
                    COLOR_MUTED,
                );

                let progress_track = RECT {
                    left: row.left + 2,
                    top: row.bottom - 3,
                    right: row.right - 2,
                    bottom: row.bottom - 1,
                };
                fill_rect(hdc, progress_track, 0x262626);
                let progress = self.download_progress(download);
                let filled = RECT {
                    left: progress_track.left,
                    top: progress_track.top,
                    right: progress_track.left
                        + ((progress_track.right - progress_track.left) as f32 * progress) as i32,
                    bottom: progress_track.bottom,
                };
                fill_rect(hdc, filled, self.accent_color);

                let cancel_glyph = if download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                    glyph(0xE74D)
                } else {
                    glyph(0xE711)
                };
                let cancel_hover =
                    self.hover_target == Some(HoverTarget::DownloadCancel(download.id));
                if cancel_hover {
                    fill_round_rect(hdc, cancel, COLOR_SURFACE_HOVER, 6);
                }
                draw_icon_glyph(
                    hdc,
                    &self.fonts.icon,
                    cancel_glyph.as_str(),
                    cancel,
                    if cancel_hover {
                        COLOR_TEXT
                    } else {
                        COLOR_MUTED
                    },
                );

                if show_pause {
                    let pause_hover =
                        self.hover_target == Some(HoverTarget::DownloadPause(download.id));
                    if pause_hover {
                        fill_round_rect(hdc, pause, COLOR_SURFACE_HOVER, 6);
                    }
                    let pause_icon = if download.paused {
                        glyph(0xE768)
                    } else {
                        glyph(0xE769)
                    };
                    draw_icon_glyph(
                        hdc,
                        &self.fonts.icon,
                        pause_icon.as_str(),
                        pause,
                        if pause_hover { COLOR_TEXT } else { COLOR_MUTED },
                    );
                }

                if download.state != COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
                    let open_hover =
                        self.hover_target == Some(HoverTarget::DownloadOpen(download.id));
                    if open_hover {
                        fill_round_rect(hdc, open, COLOR_SURFACE_HOVER, 6);
                    }
                    draw_icon_glyph(
                        hdc,
                        &self.fonts.icon,
                        glyph(0xE838).as_str(),
                        open,
                        if open_hover { COLOR_TEXT } else { COLOR_MUTED },
                    );
                }

                if index + 1 < rows.len() {
                    fill_rect(
                        hdc,
                        RECT {
                            left: row.left + 8,
                            top: row.bottom + 3,
                            right: row.right - 8,
                            bottom: row.bottom + 4,
                        },
                        0x242424,
                    );
                }
                top += 58;
            }
        }
    }

    fn paint_find_bar(&self, hdc: HDC) {
        unsafe {
            let bar = self.find_bar_rect();
            fill_round_rect(hdc, bar, 0x111111, 10);
            draw_outline(hdc, bar, 0x343434, 10);
            let input = self.find_input_rect();
            fill_round_rect(
                hdc,
                RECT {
                    left: input.left - 6,
                    top: input.top - 4,
                    right: input.right + 6,
                    bottom: input.bottom + 4,
                },
                0x080808,
                8,
            );
            let count_text = if self.find_match_count == 0 {
                "0/0".to_string()
            } else {
                format!("{}/{}", self.find_current_match, self.find_match_count)
            };
            draw_centered_text(
                hdc,
                &self.fonts.small,
                &count_text,
                RECT {
                    left: input.right + 8,
                    top: bar.top,
                    right: input.right + 50,
                    bottom: bar.bottom,
                },
                COLOR_MUTED,
            );
            let prev = self.find_prev_rect();
            let next = self.find_next_rect();
            let close = self.find_close_rect();
            if self.hover_target == Some(HoverTarget::FindPrev) {
                fill_round_rect(hdc, prev, COLOR_SURFACE_HOVER, 7);
            }
            if self.hover_target == Some(HoverTarget::FindNext) {
                fill_round_rect(hdc, next, COLOR_SURFACE_HOVER, 7);
            }
            if self.hover_target == Some(HoverTarget::FindClose) {
                fill_round_rect(hdc, close, COLOR_SURFACE_HOVER, 7);
            }
            draw_icon_glyph(
                hdc,
                &self.fonts.toolbar_icon,
                glyph(0xE70E).as_str(),
                prev,
                COLOR_MUTED,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.toolbar_icon,
                glyph(0xE70D).as_str(),
                next,
                COLOR_MUTED,
            );
            draw_icon_glyph(
                hdc,
                &self.fonts.icon,
                glyph(0xE711).as_str(),
                close,
                COLOR_MUTED,
            );
        }
    }

    fn paint_tab_favicon(&self, hdc: HDC, rect: RECT, tab: &Tab, dimmed: bool) {
        unsafe {
            if let Some(favicon) = tab.favicon_bitmap.as_ref() {
                draw_bitmap_fit(hdc, rect, favicon, dimmed);
                return;
            }
            let host = display_host(&tab.url);
            if let Some(favicon) = self.favicon_cache.get(&host) {
                draw_bitmap_fit(hdc, rect, favicon, dimmed);
                return;
            }
            draw_tab_favicon(hdc, &self.fonts.small, rect, tab, dimmed);
        }
    }

    fn paint_url_favicon(&self, hdc: HDC, rect: RECT, url: &str, dimmed: bool) -> bool {
        let host = display_host(url);
        if host.is_empty() {
            return false;
        }
        if let Some(favicon) = self.favicon_cache.get(&host) {
            unsafe {
                draw_bitmap_fit(hdc, rect, favicon, dimmed);
            }
            return true;
        }
        false
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
            draw_outline(hdc, panel, self.accent_color, 14);

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
            for (i, (tab_index, title, url)) in suggestions
                .into_iter()
                .skip(self.command_scroll_offset)
                .take(6)
                .enumerate()
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
                    fill_round_rect(hdc, indicator, self.accent_color, 2);
                }
                let favicon = RECT {
                    left: row.left + 14,
                    top: row.top + 8,
                    right: row.left + 30,
                    bottom: row.top + 24,
                };
                let mut favicon_drawn = false;
                if let Some(index) = tab_index.and_then(|index| self.tabs.get(index)) {
                    self.paint_tab_favicon(hdc, favicon, index, false);
                    favicon_drawn = true;
                } else if self.paint_url_favicon(hdc, favicon, &url, false) {
                    favicon_drawn = true;
                } else {
                    let host = display_host(&url);
                    if !host.is_empty() {
                        if let Some(matching_tab) = self
                            .tabs
                            .iter()
                            .find(|t| t.favicon_bitmap.is_some() && display_host(&t.url) == host)
                        {
                            self.paint_tab_favicon(hdc, favicon, matching_tab, false);
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
                        self.accent_color,
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

            if total_rows > 6 {
                let visible_ratio = 6.0 / total_rows as f32;
                let scroll_ratio = self.command_scroll_offset as f32 / total_rows as f32;
                let max_rows = 6;
                let track_height = (max_rows * 38) as f32;
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
            draw_icon_glyph(
                hdc,
                &self.fonts.toolbar_icon,
                glyph(0xE718).as_str(),
                RECT {
                    left: 22,
                    top: rect.top + 4,
                    right: 22 + 20,
                    bottom: rect.top + 4 + 18,
                },
                self.accent_color,
            );
            fill_rect(
                hdc,
                RECT {
                    left: 22,
                    top: rect.top + 28,
                    right: self.sidebar_width() - 18,
                    bottom: rect.top + 29,
                },
                0x2a2a2a,
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
        let is_ghost = self.is_preview_item(SidebarRow::Folder(folder_id));
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
            let icon_color = if is_ghost { 0x555555 } else { COLOR_MUTED };
            if is_ghost {
                fill_round_rect(hdc, item, 0x0f0f0f, 8);
                draw_outline(hdc, item, 0x333333, 8);
            } else if is_renaming {
                fill_round_rect(hdc, item, 0x242424, 8);
            } else if self.hover_folder == Some(folder_id) {
                fill_round_rect(hdc, item, self.secondary_color, 8);
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
                icon_color,
            );
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
                icon_color,
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
                    icon_color,
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
                            if active {
                                self.accent_color
                            } else {
                                self.secondary_color
                            },
                            14,
                        );
                        draw_outline(
                            hdc,
                            rect,
                            if active { self.accent_color } else { 0x2f2f2f },
                            14,
                        );
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
                        fill_rect(hdc, line_rect, self.accent_color);
                    } else {
                        let width = self.sidebar_width() as i32;
                        let line_rect = RECT {
                            left: 14,
                            top: self.sidebar_rows_top() - 2,
                            right: width - 14,
                            bottom: self.sidebar_rows_top(),
                        };
                        fill_rect(hdc, line_rect, self.accent_color);
                    }
                }
                Some(DropTarget::UnpinnedSection) => {
                    let rects = self.sidebar_row_rects();
                    if let Some((divider_index, (_, divider_rect))) = rects
                        .iter()
                        .enumerate()
                        .find(|(_, (row, _))| matches!(row, SidebarRow::Label(SidebarLabel::Tabs)))
                    {
                        let target_y = rects
                            .iter()
                            .skip(divider_index + 1)
                            .rev()
                            .find(|(row, _)| !matches!(row, SidebarRow::Label(_)))
                            .map(|(_, rect)| rect.bottom)
                            .unwrap_or(divider_rect.bottom);
                        let width = self.sidebar_width();
                        fill_rect(
                            hdc,
                            RECT {
                                left: 14,
                                top: target_y - 2,
                                right: width - 14,
                                bottom: target_y,
                            },
                            self.accent_color,
                        );
                    }
                }
                Some(DropTarget::RootAfter { row, .. }) => {
                    let rects = self.sidebar_row_rects();
                    let target_y = row
                        .and_then(|target| {
                            rects
                                .iter()
                                .find(|(candidate, _)| *candidate == target)
                                .map(|(_, rect)| rect.bottom)
                        })
                        .unwrap_or_else(|| {
                            rects
                                .iter()
                                .rev()
                                .find(|(candidate, _)| !matches!(candidate, SidebarRow::Label(_)))
                                .map(|(_, rect)| rect.bottom)
                                .unwrap_or(self.sidebar_rows_top())
                        });
                    let width = self.sidebar_width();
                    fill_rect(
                        hdc,
                        RECT {
                            left: 14,
                            top: target_y - 2,
                            right: width - 14,
                            bottom: target_y,
                        },
                        self.accent_color,
                    );
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
                        fill_rect(hdc, line_rect, self.accent_color);
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
                        fill_rect(hdc, line_rect, self.accent_color);
                    }
                }
                Some(DropTarget::None) | None => {}
            }
        }
    }

    fn paint_split_drop_overlay(&self, hdc: HDC) {
        unsafe {
            let Some(rect) = self.split_placeholder_rect() else {
                return;
            };
            if rect.right <= rect.left || rect.bottom <= rect.top {
                return;
            }
            let inset = RECT {
                left: rect.left + 10,
                top: rect.top + 10,
                right: rect.right - 10,
                bottom: rect.bottom - 10,
            };
            fill_round_rect(hdc, inset, self.secondary_color, 12);
            draw_outline(hdc, inset, self.accent_color, 12);
            let label = match self.split_drop_target.map(|target| target.zone) {
                Some(SplitDropZone::Left) => "Split left",
                Some(SplitDropZone::Right) => "Split right",
                Some(SplitDropZone::Top) => "Split top",
                Some(SplitDropZone::Bottom) => "Split bottom",
                None => "Split",
            };
            let mut label_rect = inset;
            label_rect.bottom = ((inset.top + inset.bottom) / 2).min(inset.top + 42);
            draw_centered_text(hdc, &self.fonts.body, label, label_rect, COLOR_TEXT);
            if let Some(DragState {
                source: DragSource::Tab(index),
                ..
            }) = self.drag_state
            {
                if let Some(tab) = self.tabs.get(index) {
                    let title = if tab.title.trim().is_empty() {
                        label_for_url(&tab.url)
                    } else {
                        tab.title.clone()
                    };
                    let icon = RECT {
                        left: inset.left + 18,
                        top: label_rect.bottom + 10,
                        right: inset.left + 38,
                        bottom: label_rect.bottom + 30,
                    };
                    self.paint_tab_favicon(hdc, icon, tab, false);
                    draw_text(
                        hdc,
                        &self.fonts.body,
                        &title,
                        RECT {
                            left: icon.right + 10,
                            top: icon.top - 6,
                            right: inset.right - 18,
                            bottom: icon.bottom + 6,
                        },
                        COLOR_TEXT,
                    );
                }
            }
        }
    }

    fn paint_split_toolbar(&self, hdc: HDC) {
        unsafe {
            let Some(toolbar) = self.split_toolbar_rect() else {
                return;
            };
            fill_round_rect(hdc, toolbar, self.secondary_color, 10);
            draw_outline(hdc, toolbar, COLOR_BORDER, 10);
            if let Some(layout_button) = self.split_layout_button_rect() {
                if self.hover_target == Some(HoverTarget::SplitLayout) {
                    fill_round_rect(hdc, layout_button, COLOR_SURFACE_HOVER, 8);
                }
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    glyph(0xE9F9).as_str(),
                    layout_button,
                    COLOR_TEXT,
                );
            }
            if let Some(group_id) = self.active_split_group_id() {
                if let Some(group) = self.split_group(group_id) {
                    let label = match group.layout {
                        SplitLayout::Horizontal => "H",
                        SplitLayout::Vertical => "V",
                        SplitLayout::Grid => "G",
                    };
                    draw_centered_text(
                        hdc,
                        &self.fonts.small,
                        label,
                        RECT {
                            left: toolbar.left + 36,
                            top: toolbar.top,
                            right: toolbar.right - 36,
                            bottom: toolbar.bottom,
                        },
                        COLOR_MUTED,
                    );
                }
            }
            if let Some(unsplit_button) = self.split_unsplit_button_rect() {
                if self.hover_target == Some(HoverTarget::SplitUnsplit) {
                    fill_round_rect(hdc, unsplit_button, COLOR_SURFACE_HOVER, 8);
                }
                draw_icon_glyph(
                    hdc,
                    &self.fonts.toolbar_icon,
                    glyph(0xE711).as_str(),
                    unsplit_button,
                    COLOR_TEXT,
                );
            }
        }
    }

    fn paint_tab(&self, hdc: HDC, index: usize, tab: &Tab, item: RECT, force_ghost: bool) {
        unsafe {
            let mut item = item;
            let is_ghost = force_ghost || self.is_preview_item(SidebarRow::Tab(index));
            let depth = self.tab_depth(index);
            if depth > 0 {
                item.left += (depth * 16) as i32;
            }
            if is_ghost {
                fill_round_rect(hdc, item, 0x0f0f0f, 10);
                draw_outline(hdc, item, 0x333333, 10);
            } else if self.hover_tab == Some(index) || Some(index) == self.active_tab_index() {
                fill_round_rect(hdc, item, self.secondary_color, 10);
            }
            let text_left = item.left + 40;
            let favicon_left = item.left + 12;
            let favicon = RECT {
                left: favicon_left,
                top: item.top + 11,
                right: favicon_left + 18,
                bottom: item.top + 29,
            };
            self.paint_tab_favicon(hdc, favicon, tab, is_ghost);
            let text_color = if is_ghost {
                0x555555
            } else if Some(index) == self.active_tab_index() {
                COLOR_TEXT
            } else {
                COLOR_MUTED
            };
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
                text_color,
            );
            if !is_ghost {
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
    }

    fn paint_split_group_row(&self, hdc: HDC, group_id: usize, item: RECT) {
        unsafe {
            let Some(group) = self.split_group(group_id) else {
                return;
            };
            let active_index = self.split_group_active_index(group_id);
            let active = active_index.and_then(|index| self.tabs.get(index));
            let title = group
                .tab_ids
                .iter()
                .filter_map(|tab_id| {
                    self.tabs
                        .iter()
                        .find(|tab| tab.id == *tab_id)
                        .map(|tab| {
                            if tab.title.trim().is_empty() {
                                label_for_url(&tab.url)
                            } else {
                                tab.title.clone()
                            }
                        })
                })
                .take(3)
                .collect::<Vec<_>>()
                .join(" + ");
            let title = if title.trim().is_empty() {
                "Split View".to_string()
            } else {
                title
            };
            let is_active = group.tab_ids.iter().any(|tab_id| {
                self.tabs
                    .get(self.active)
                    .map(|tab| tab.id == *tab_id)
                    .unwrap_or(false)
            });
            let is_hovered = active_index
                .map(|index| self.hover_tab == Some(index))
                .unwrap_or(false);
            let row = item;
            if is_active || is_hovered {
                fill_round_rect(hdc, row, self.secondary_color, 10);
            }
            let accent = RECT {
                left: row.left + 8,
                top: row.top + 8,
                right: row.left + 12,
                bottom: row.bottom - 8,
            };
            fill_round_rect(hdc, accent, self.accent_color, 3);
            let mini = RECT {
                left: row.left + 20,
                top: row.top + 8,
                right: row.left + 58,
                bottom: row.bottom - 8,
            };
            fill_round_rect(hdc, mini, 0x101010, 6);
            draw_outline(hdc, mini, COLOR_BORDER, 6);
            let mini_cells = self.split_layout_rects_for(
                RECT {
                    left: mini.left + 4,
                    top: mini.top + 4,
                    right: mini.right - 4,
                    bottom: mini.bottom - 4,
                },
                group.layout,
                group.tab_ids.len(),
            );
            for (tab_id, cell) in group.tab_ids.iter().copied().zip(mini_cells) {
                let is_active_cell = active
                    .map(|tab| tab.id == tab_id)
                    .unwrap_or(false);
                fill_round_rect(
                    hdc,
                    cell,
                    if is_active_cell {
                        self.accent_color
                    } else {
                        self.secondary_color
                    },
                    4,
                );
                if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
                    let icon = RECT {
                        left: cell.left + 2,
                        top: cell.top + 2,
                        right: cell.right - 2,
                        bottom: cell.bottom - 2,
                    };
                    self.paint_tab_favicon(hdc, icon, tab, !is_active_cell);
                }
            }
            draw_text(
                hdc,
                &self.fonts.body,
                &title,
                RECT {
                    left: row.left + 68,
                    top: row.top + 2,
                    right: row.right - 42,
                    bottom: row.top + 29,
                },
                if is_active { COLOR_TEXT } else { COLOR_MUTED },
            );
            let subtitle = format!(
                "{} panes - {}",
                group.tab_ids.len(),
                match group.layout {
                    SplitLayout::Horizontal => "horizontal",
                    SplitLayout::Vertical => "vertical",
                    SplitLayout::Grid => "grid",
                }
            );
            draw_text(
                hdc,
                &self.fonts.small,
                &subtitle,
                RECT {
                    left: row.left + 68,
                    top: row.top + 25,
                    right: row.right - 42,
                    bottom: row.bottom,
                },
                COLOR_MUTED,
            );
            if let Some(tab) = active {
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
                        self.tab_audio_rect(row),
                        if tab.muted { COLOR_MUTED } else { COLOR_TEXT },
                    );
                }
            }
            if is_hovered {
                draw_icon_glyph(
                    hdc,
                    &self.fonts.icon,
                    glyph(0xE711).as_str(),
                    RECT {
                        left: row.right - 30,
                        top: row.top + 12,
                        right: row.right - 8,
                        bottom: row.top + 34,
                    },
                    if active_index
                        .map(|index| self.hover_close == Some(index))
                        .unwrap_or(false)
                    {
                        COLOR_TEXT
                    } else {
                        COLOR_MUTED
                    },
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

        if self.show_default_bubble && self.sidebar_width() >= 240 {
            if let Some(close_rect) = self.default_bubble_close_rect() {
                if point_in_rect(x, y, close_rect) {
                    self.show_default_bubble = false;
                    self.default_bubble_dismissed = true;
                    self.set_sidebar_mode(SidebarMode::Hidden);
                    self.save_state();
                    self.refresh();
                    return;
                }
            }
            if let Some(btn_rect) = self.default_bubble_button_rect() {
                if point_in_rect(x, y, btn_rect) {
                    make_aster_default_browser();
                    self.refresh();
                    return;
                }
            }
            if let Some(bubble_rect) = self.default_bubble_rect() {
                if point_in_rect(x, y, bubble_rect) {
                    return;
                }
            }
        }

        if let Some(action) = self.download_action_at(x, y) {
            self.run_download_action(action);
            return;
        }

        for (target, rect) in self.download_indicator_rects() {
            if point_in_rect(x, y, rect) {
                let new_mode = match target {
                    Some(id) => DownloadPanelMode::Single(id),
                    None => DownloadPanelMode::All,
                };
                if self.download_panel == Some(new_mode) && self.download_panel_reveal > 0.99 {
                    return;
                }
                self.download_panel = Some(new_mode);
                self.download_panel_reveal = 0.0;
                self.download_panel_reveal_target = 1.0;
                self.ensure_download_timer();
                self.refresh();
                return;
            }
        }

        if self.download_panel.is_some()
            && self.download_panel_reveal > 0.01
            && !self
                .download_panel_rect()
                .map(|rect| point_in_rect(x, y, rect))
                .unwrap_or(false)
        {
            self.download_panel_reveal_target = 0.0;
            self.ensure_download_timer();
            self.refresh();
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
            self.settings_open = false;
            self.mode_menu_open = false;
            self.set_sidebar_mode(SidebarMode::Hidden);
            return;
        }

        if self.settings_open {
            if point_in_rect(x, y, self.extension_store_row_rect()) {
                self.settings_open = false;
                self.mode_menu_open = false;
                self.open_extension_store();
                return;
            }
            if point_in_rect(x, y, self.extensions_page_row_rect()) {
                self.settings_open = false;
                self.mode_menu_open = false;
                self.open_extensions_page();
                return;
            }
            if point_in_rect(x, y, self.history_page_row_rect()) {
                self.settings_open = false;
                self.mode_menu_open = false;
                self.open_history_page();
                return;
            }
            if point_in_rect(x, y, self.settings_page_row_rect()) {
                self.settings_open = false;
                self.mode_menu_open = false;
                self.open_settings_page();
                return;
            }
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
                let _ =
                    WindowsAndMessaging::ShowWindow(self.hwnd, WindowsAndMessaging::SW_MINIMIZE);
            }
            return;
        }
        if point_in_rect(x, y, max_btn) {
            unsafe {
                if WindowsAndMessaging::IsZoomed(self.hwnd).as_bool() {
                    let _ =
                        WindowsAndMessaging::ShowWindow(self.hwnd, WindowsAndMessaging::SW_RESTORE);
                } else {
                    let _ = WindowsAndMessaging::ShowWindow(
                        self.hwnd,
                        WindowsAndMessaging::SW_MAXIMIZE,
                    );
                }
            }
            return;
        }
        if point_in_rect(x, y, close_btn) {
            unsafe {
                let _ = WindowsAndMessaging::PostMessageW(
                    Some(self.hwnd),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
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

        if point_in_rect(x, y, self.extension_button_rect()) {
            self.open_extension_menu();
            return;
        }

        if point_in_rect(x, y, self.address_menu_rect()) {
            let rect = self.address_menu_rect();
            self.open_address_menu(rect.left, rect.bottom + 4);
            return;
        }

        if let Some(rect) = self.split_layout_button_rect() {
            if point_in_rect(x, y, rect) {
                self.cycle_active_split_layout();
                return;
            }
        }

        if let Some(rect) = self.split_unsplit_button_rect() {
            if point_in_rect(x, y, rect) {
                if let Some(group_id) = self.active_split_group_id() {
                    self.unsplit_group(group_id);
                }
                return;
            }
        }

        if self.find_open {
            if point_in_rect(x, y, self.find_prev_rect()) {
                self.run_find_script(-1);
                return;
            }
            if point_in_rect(x, y, self.find_next_rect()) {
                self.run_find_script(1);
                return;
            }
            if point_in_rect(x, y, self.find_close_rect()) {
                self.close_find_bar();
                return;
            }
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
                    let row = self.sidebar_row_rect_for_tab(index);
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
            SidebarHit::WorkspaceHeader
            | SidebarHit::WorkspaceButton(_)
            | SidebarHit::PinnedSection => {
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
                            if is_pinned {
                                MENU_FOLDER_UNPIN
                            } else {
                                MENU_FOLDER_PIN
                            },
                            if is_pinned {
                                "Unpin Folder"
                            } else {
                                "Pin Folder"
                            },
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
                labels.push((MENU_TAB_DUPLICATE, "Duplicate Tab".to_string()));
                if tab.split_group_id.is_some() {
                    labels.push((MENU_TAB_UNSPLIT, "Unsplit Tab".to_string()));
                    labels.push((MENU_TAB_SPLIT_HORIZONTAL, "Horizontal Layout".to_string()));
                    labels.push((MENU_TAB_SPLIT_VERTICAL, "Vertical Layout".to_string()));
                    labels.push((MENU_TAB_SPLIT_GRID, "Grid Layout".to_string()));
                } else if self
                    .active_tab_index()
                    .map(|active| active != index)
                    .unwrap_or(false)
                    || self.active_workspace_tabs().len() > 1
                {
                    labels.push((MENU_TAB_SPLIT_HORIZONTAL, "Split Horizontally".to_string()));
                    labels.push((MENU_TAB_SPLIT_VERTICAL, "Split Vertically".to_string()));
                    labels.push((MENU_TAB_SPLIT_GRID, "Split as Grid".to_string()));
                }
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
        let mut items = vec![
            menu_item(MENU_TAB_NEW, "New Tab"),
            menu_item(MENU_WORKSPACE_NEW_FOLDER, "New Folder"),
            menu_item(MENU_WORKSPACE_NEW, "New Workspace"),
            menu_item(MENU_WORKSPACE_RENAME, "Rename Workspace"),
        ];
        if !self.closed_tabs.is_empty() {
            items.push(menu_item(MENU_REOPEN_CLOSED, "Reopen Last Closed Tab"));
            for (offset, closed) in self.closed_tabs.iter().rev().take(5).enumerate() {
                let title = if closed.title.trim().is_empty() {
                    label_for_url(&closed.url)
                } else {
                    closed.title.clone()
                };
                items.push(menu_item_with_subtitle(
                    MENU_RECENTLY_CLOSED_BASE + offset,
                    &title,
                    &closed.url,
                ));
            }
        }
        self.open_overlay_menu(x, y, MenuTarget::SidebarBlank, items);
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
                let _ = WindowsAndMessaging::ShowWindow(
                    self.overlay_menu_hwnd,
                    WindowsAndMessaging::SW_HIDE,
                );
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
            let _ = WindowsAndMessaging::ShowWindow(
                self.overlay_menu_hwnd,
                WindowsAndMessaging::SW_SHOW,
            );
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
            let _ = WindowsAndMessaging::ShowWindow(
                self.overlay_menu_hwnd,
                WindowsAndMessaging::SW_HIDE,
            );
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
            MenuTarget::AddressMenu => self.run_address_menu_command(id),
            MenuTarget::Bookmarks => {
                let offset = id.saturating_sub(MENU_BOOKMARK_OPEN_BASE);
                if let Some(bookmark) = self.bookmarks.get(offset) {
                    let url = bookmark.url.clone();
                    self.navigate_active(&url);
                }
            }
            MenuTarget::Extensions => self.run_extension_menu_command(id),
        }
        self.refresh();
    }

    fn run_extension_menu_command(&mut self, id: usize) {
        match id {
            MENU_EXTENSION_MANAGE => self.open_extensions_page(),
            MENU_EXTENSION_STORE => self.open_extension_store(),
            MENU_EXTENSION_REFRESH => {
                self.extension_notice = Some("Refreshing extensions...".to_string());
                self.refresh_extensions();
            }
            id if (MENU_EXTENSION_ITEM_BASE..MENU_EXTENSION_ITEM_BASE + 12).contains(&id) => {
                self.open_extensions_page();
            }
            _ => {}
        }
    }

    fn run_address_menu_command(&mut self, id: usize) {
        match id {
            MENU_ADDRESS_BOOKMARK => self.toggle_active_bookmark(),
            MENU_ADDRESS_BOOKMARKS => {
                let rect = self.address_pill_rect();
                self.open_bookmarks_menu(rect.right - MENU_WIDTH, rect.bottom + 8);
            }
            MENU_ADDRESS_ZOOM_OUT => self.adjust_active_zoom(-0.1),
            MENU_ADDRESS_ZOOM_RESET => self.reset_active_zoom(),
            MENU_ADDRESS_ZOOM_IN => self.adjust_active_zoom(0.1),
            MENU_ADDRESS_CLEAR_RELOAD => self.clear_site_data_and_reload(),
            _ => {}
        }
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
            MENU_REOPEN_CLOSED => self.reopen_closed_tab(),
            id if (MENU_RECENTLY_CLOSED_BASE..MENU_RECENTLY_CLOSED_BASE + 20).contains(&id) => {
                self.reopen_closed_tab_at(id - MENU_RECENTLY_CLOSED_BASE);
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
            MENU_TAB_PIN
            | MENU_TAB_UNPIN
            | MENU_TAB_REMOVE_FOLDER
            | MENU_TAB_DUPLICATE
            | MENU_TAB_CLOSE
            | MENU_TAB_DELETE_PIN
            | MENU_TAB_SPLIT_WITH_ACTIVE
            | MENU_TAB_SPLIT_NEXT
            | MENU_TAB_UNSPLIT
            | MENU_TAB_SPLIT_HORIZONTAL
            | MENU_TAB_SPLIT_VERTICAL
            | MENU_TAB_SPLIT_GRID
                if matches!(hit, SidebarHit::Tab(_)) =>
            {
                if let SidebarHit::Tab(index) = hit {
                    match id {
                        MENU_TAB_PIN => {
                            if let Some(tab_id) = self.tabs.get(index).map(|tab| tab.id) {
                                self.detach_tab_from_split_by_id(tab_id);
                            }
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.pinned = true;
                                tab.pinned_url = Some(tab.url.clone());
                                tab.folder_id = None;
                            }
                        }
                        MENU_TAB_UNPIN => {
                            if let Some(tab_id) = self.tabs.get(index).map(|tab| tab.id) {
                                self.detach_tab_from_split_by_id(tab_id);
                            }
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.pinned = false;
                                tab.pinned_url = None;
                            }
                        }
                        MENU_TAB_REMOVE_FOLDER => {
                            if let Some(tab_id) = self.tabs.get(index).map(|tab| tab.id) {
                                self.detach_tab_from_split_by_id(tab_id);
                            }
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.folder_id = None;
                            }
                        }
                        MENU_TAB_DUPLICATE => {
                            let _ = self.duplicate_tab(index, true);
                        }
                        MENU_TAB_SPLIT_WITH_ACTIVE | MENU_TAB_SPLIT_NEXT => {
                            self.split_tab_with_active(index, SplitLayout::Horizontal);
                        }
                        MENU_TAB_UNSPLIT => {
                            if let Some(tab_id) = self.tabs.get(index).map(|tab| tab.id) {
                                self.detach_tab_from_split_by_id(tab_id);
                                self.layout();
                                self.refresh();
                            }
                        }
                        MENU_TAB_SPLIT_HORIZONTAL => {
                            if let Some(group_id) =
                                self.tabs.get(index).and_then(|tab| tab.split_group_id)
                            {
                                if let Some(group) =
                                    self.split_groups.iter_mut().find(|group| group.id == group_id)
                                {
                                    group.layout = SplitLayout::Horizontal;
                                }
                                self.layout();
                                self.refresh();
                            } else {
                                self.split_tab_with_active(index, SplitLayout::Horizontal);
                            }
                        }
                        MENU_TAB_SPLIT_VERTICAL => {
                            if let Some(group_id) =
                                self.tabs.get(index).and_then(|tab| tab.split_group_id)
                            {
                                if let Some(group) =
                                    self.split_groups.iter_mut().find(|group| group.id == group_id)
                                {
                                    group.layout = SplitLayout::Vertical;
                                }
                                self.layout();
                                self.refresh();
                            } else {
                                self.split_tab_with_active(index, SplitLayout::Vertical);
                            }
                        }
                        MENU_TAB_SPLIT_GRID => {
                            if let Some(group_id) =
                                self.tabs.get(index).and_then(|tab| tab.split_group_id)
                            {
                                if let Some(group) =
                                    self.split_groups.iter_mut().find(|group| group.id == group_id)
                                {
                                    group.layout = SplitLayout::Grid;
                                }
                                self.layout();
                                self.refresh();
                            } else {
                                self.split_tab_with_active(index, SplitLayout::Grid);
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
                            if let Some(tab_id) = self.tabs.get(index).map(|tab| tab.id) {
                                self.detach_tab_from_split_by_id(tab_id);
                            }
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
                let mut screen_pt = POINT {
                    x: cx + 10,
                    y: cy + 10,
                };
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
        let old_drop_target = self.drop_target;
        let old_split_drop_target = self.split_drop_target;
        self.hover_close = None;
        self.hover_tab = None;
        self.hover_folder = None;
        self.hover_target = None;
        self.drop_target = Some(DropTarget::None);
        self.split_drop_target = None;

        if self.show_default_bubble && self.sidebar_width() >= 240 {
            if let Some(close_rect) = self.default_bubble_close_rect() {
                if point_in_rect(x, y, close_rect) {
                    self.hover_target = Some(HoverTarget::DefaultBubbleClose);
                }
            }
            if self.hover_target.is_none() {
                if let Some(btn_rect) = self.default_bubble_button_rect() {
                    if point_in_rect(x, y, btn_rect) {
                        self.hover_target = Some(HoverTarget::DefaultBubbleSetDefault);
                    }
                }
            }
        }

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

        if self.download_panel.is_some() {
            if let Some(panel) = self.download_panel_rect() {
                let mut top = panel.top + 9;
                for download in self.download_panel_rows() {
                    let row = RECT {
                        left: panel.left + 12,
                        top,
                        right: panel.right - 12,
                        bottom: top + 50,
                    };
                    let cancel = RECT {
                        left: row.right - 22,
                        top: row.top + 4,
                        right: row.right,
                        bottom: row.top + 26,
                    };
                    let open = RECT {
                        left: row.right - 50,
                        top: row.top + 4,
                        right: row.right - 28,
                        bottom: row.top + 26,
                    };
                    let show_pause = download.state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
                    let pause = RECT {
                        left: row.right - 78,
                        top: row.top + 4,
                        right: row.right - 56,
                        bottom: row.top + 26,
                    };
                    if point_in_rect(x, y, cancel) {
                        self.hover_target = Some(HoverTarget::DownloadCancel(download.id));
                        break;
                    }
                    if download.state != COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED
                        && point_in_rect(x, y, open)
                    {
                        self.hover_target = Some(HoverTarget::DownloadOpen(download.id));
                        break;
                    }
                    if show_pause && point_in_rect(x, y, pause) {
                        self.hover_target = Some(HoverTarget::DownloadPause(download.id));
                        break;
                    }
                    top += 58;
                }
            }
        }

        if self.hover_target.is_none() {
            if self.find_open && point_in_rect(x, y, self.find_prev_rect()) {
                self.hover_target = Some(HoverTarget::FindPrev);
            } else if self.find_open && point_in_rect(x, y, self.find_next_rect()) {
                self.hover_target = Some(HoverTarget::FindNext);
            } else if self.find_open && point_in_rect(x, y, self.find_close_rect()) {
                self.hover_target = Some(HoverTarget::FindClose);
            } else if point_in_rect(x, y, self.logo_rect()) {
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
                } else if point_in_rect(x, y, self.extension_button_rect()) {
                    self.hover_target = Some(HoverTarget::ExtensionsButton);
                } else if point_in_rect(x, y, self.address_menu_rect()) {
                    self.hover_target = Some(HoverTarget::AddressMenu);
                } else if self
                    .split_layout_button_rect()
                    .map(|rect| point_in_rect(x, y, rect))
                    .unwrap_or(false)
                {
                    self.hover_target = Some(HoverTarget::SplitLayout);
                } else if self
                    .split_unsplit_button_rect()
                    .map(|rect| point_in_rect(x, y, rect))
                    .unwrap_or(false)
                {
                    self.hover_target = Some(HoverTarget::SplitUnsplit);
                } else if point_in_rect(x, y, self.address_pill_rect()) {
                    self.hover_target = Some(HoverTarget::Address);
                } else if point_in_rect(x, y, self.settings_rect()) {
                    self.hover_target = Some(HoverTarget::Settings);
                } else if let Some((target, _)) = self
                    .download_indicator_rects()
                    .into_iter()
                    .find(|(_, rect)| point_in_rect(x, y, *rect))
                {
                    self.hover_target = match target {
                        Some(id) => Some(HoverTarget::DownloadIndicator(id)),
                        None => Some(HoverTarget::DownloadOverflow),
                    };
                } else if self.settings_open
                    && point_in_rect(x, y, self.extension_store_row_rect())
                {
                    self.hover_target = Some(HoverTarget::ExtensionStore);
                    self.mode_menu_open = false;
                } else if self.settings_open
                    && point_in_rect(x, y, self.extensions_page_row_rect())
                {
                    self.hover_target = Some(HoverTarget::ExtensionsPage);
                    self.mode_menu_open = false;
                } else if self.settings_open && point_in_rect(x, y, self.history_page_row_rect()) {
                    self.hover_target = Some(HoverTarget::HistoryPage);
                    self.mode_menu_open = false;
                } else if self.settings_open && point_in_rect(x, y, self.settings_page_row_rect()) {
                    self.hover_target = Some(HoverTarget::SettingsPage);
                    self.mode_menu_open = false;
                } else if self.settings_open && point_in_rect(x, y, self.mode_row_rect()) {
                    self.hover_target = Some(HoverTarget::ModeRow);
                    self.mode_menu_open = true;
                } else if self.settings_open && point_in_rect(x, y, self.mode_options_rect()) {
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
        }

        if self.settings_open
            && self.mode_menu_open
            && !point_in_rect(x, y, self.mode_row_rect())
            && !point_in_rect(x, y, self.mode_options_rect())
        {
            self.mode_menu_open = false;
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

        if self.show_default_bubble && self.sidebar_width() >= 240 {
            if let Some(bubble_rect) = self.default_bubble_rect() {
                if point_in_rect(x, y, bubble_rect) {
                    self.hover_tab = None;
                    self.hover_close = None;
                    self.hover_folder = None;
                }
            }
        }

        if self.drag_state.as_ref().map(|d| d.active).unwrap_or(false) {
            let split_target = self.calculate_split_drop_target(x, y);
            // Temporarily disable drag preview so that calculate_drop_target
            // sees the raw (non-preview) sidebar layout, where the pinned
            // section divider is at its true position. Without this, the
            // preview shifts the dragged item into the unpinned section,
            // causing the divider to move up and making y <= divider_y fail.
            if let Some(ref mut d) = self.drag_state {
                d.active = false;
            }
            self.drop_target = Some(self.calculate_drop_target(x, y));
            self.split_drop_target = split_target;
            if self.split_drop_target.is_some() {
                self.drop_target = Some(DropTarget::None);
            }
            if let Some(ref mut d) = self.drag_state {
                d.active = true;
            }
        }

        if old_split_drop_target != self.split_drop_target {
            self.layout();
        }

        if !self.animating_sidebar
            && (old_close != self.hover_close
                || old_tab != self.hover_tab
                || old_folder != self.hover_folder
                || old_target != self.hover_target
                || old_mode_menu != self.mode_menu_open
                || old_hovering != self.hovering_sidebar
                || old_drop_target != self.drop_target
                || old_split_drop_target != self.split_drop_target)
        {
            self.refresh();
        }
    }

    fn start_drag_candidate(&mut self, x: i32, y: i32) -> bool {
        if self.renaming_folder_id.is_some() {
            return false;
        }
        if let Some(SidebarHit::Tab(index)) = self.hit_sidebar(x, y) {
            if let Some(row) = self.sidebar_row_rect_for_tab(index) {
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
                    return true;
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
                return true;
            }
        }
        false
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
        if !drag.active {
            self.drop_target = Some(DropTarget::None);
            self.split_drop_target = None;
            return false;
        }
        let duplicate = matches!(drag.source, DragSource::Tab(_))
            && unsafe { (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 };
        self.handle_drop(drag.source, x, y, duplicate);
        self.drop_target = Some(DropTarget::None);
        self.split_drop_target = None;
        self.layout();
        true
    }
    fn handle_drop(&mut self, source: DragSource, _x: i32, _y: i32, duplicate_tab: bool) {
        let target = self.drop_target.unwrap_or(DropTarget::None);
        let split_target = self.split_drop_target;

        match source {
            DragSource::Tab(mut from_index) => {
                if from_index >= self.tabs.len() {
                    return;
                }
                if duplicate_tab {
                    let Some(new_index) = self.duplicate_tab(from_index, true) else {
                        return;
                    };
                    from_index = new_index;
                }
                if let Some(split_target) = split_target {
                    self.apply_split_drop(from_index, split_target, None);
                    return;
                }
                if let Some(tab_id) = self.tabs.get(from_index).map(|tab| tab.id) {
                    self.detach_tab_from_split_by_id(tab_id);
                    if let Some(index) = self.tab_index_by_id(tab_id) {
                        from_index = index;
                    } else {
                        return;
                    }
                }
                let dragged_workspace = self.tabs[from_index].workspace_id;

                match target {
                    DropTarget::PinnedSection => {
                        let tab_id = self.tabs[from_index].id;
                        let mut tab = self.tabs.remove(from_index);
                        tab.pinned = true;
                        tab.pinned_url = Some(tab.url.clone());
                        tab.folder_id = None;
                        self.tabs.insert(0, tab);
                        if let Some(new_active) = self.tabs.iter().position(|t| t.id == tab_id) {
                            self.active = new_active;
                            self.place_root_row_at_start(SidebarRow::Tab(new_active), true);
                        }
                    }
                    DropTarget::Folder(folder_id) => {
                        if let Some(folder) = self.folders.iter().find(|folder| {
                            folder.id == folder_id && folder.workspace_id == dragged_workspace
                        }) {
                            if let Some(tab) = self.tabs.get_mut(from_index) {
                                tab.folder_id = Some(folder_id);
                                tab.pinned = folder.pinned;
                                tab.pinned_url = if folder.pinned {
                                    Some(tab.url.clone())
                                } else {
                                    None
                                };
                            }
                        }
                    }
                    DropTarget::Tab(target_index) if target_index < self.tabs.len() => {
                        if target_index == from_index {
                            return;
                        }
                        let target_id = self.tabs[target_index].id;
                        let target_folder = self.tabs[target_index].folder_id;
                        let target_pinned = self.tabs[target_index].pinned;
                        let tab_id = self.tabs[from_index].id;
                        let mut tab = self.tabs.remove(from_index);
                        tab.pinned = target_pinned;
                        tab.pinned_url = if target_pinned {
                            Some(tab.url.clone())
                        } else {
                            None
                        };
                        tab.folder_id = target_folder;
                        let insert_at = self
                            .tabs
                            .iter()
                            .position(|candidate| candidate.id == target_id)
                            .unwrap_or_else(|| target_index.min(self.tabs.len()));
                        self.tabs.insert((insert_at + 1).min(self.tabs.len()), tab);
                        if let Some(new_active) = self.tabs.iter().position(|tab| tab.id == tab_id)
                        {
                            self.active = new_active;
                            let target_row = self
                                .tabs
                                .iter()
                                .position(|tab| tab.id == target_id)
                                .map(SidebarRow::Tab);
                            self.place_root_row_after(
                                SidebarRow::Tab(new_active),
                                target_pinned,
                                target_row,
                            );
                        }
                    }
                    DropTarget::UnpinnedSection => {
                        let mut tab = self.tabs.remove(from_index);
                        tab.pinned = false;
                        tab.pinned_url = None;
                        tab.folder_id = None;
                        self.tabs.push(tab);
                        let new_index = self.tabs.len().saturating_sub(1);
                        self.place_root_row_after(SidebarRow::Tab(new_index), false, None);
                    }
                    DropTarget::RootAfter { pinned, row } => {
                        let tab_id = self.tabs[from_index].id;
                        let mut tab = self.tabs.remove(from_index);
                        tab.pinned = pinned;
                        tab.pinned_url = if pinned { Some(tab.url.clone()) } else { None };
                        tab.folder_id = None;
                        self.tabs.push(tab);
                        if let Some(new_active) = self.tabs.iter().position(|tab| tab.id == tab_id)
                        {
                            self.active = new_active;
                            self.place_root_row_after(SidebarRow::Tab(new_active), pinned, row);
                        }
                    }
                    _ => {}
                }
            }
            DragSource::Folder(from_folder_id) => match target {
                DropTarget::PinnedSection => {
                    if let Some(pos) = self.folders.iter().position(|f| f.id == from_folder_id) {
                        let mut folder = self.folders.remove(pos);
                        folder.pinned = true;
                        folder.parent_id = None;
                        self.folders.insert(pos.min(self.folders.len()), folder);
                        self.place_root_row_at_start(SidebarRow::Folder(from_folder_id), true);
                    }
                    self.propagate_folder_pinning(from_folder_id, true);
                }
                DropTarget::Folder(target_folder_id) => {
                    if target_folder_id == from_folder_id
                        || self.is_descendant_of(target_folder_id, from_folder_id)
                        || self.is_descendant_of(from_folder_id, target_folder_id)
                    {
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
                DropTarget::Tab(target_tab_index) => {
                    let tab_pinned = self
                        .tabs
                        .get(target_tab_index)
                        .map(|t| t.pinned)
                        .unwrap_or(false);
                    if let Some(pos) = self.folders.iter().position(|f| f.id == from_folder_id) {
                        let mut folder = self.folders.remove(pos);
                        folder.pinned = tab_pinned;
                        folder.parent_id = None;
                        self.folders.insert(pos.min(self.folders.len()), folder);
                        self.place_root_row_after(
                            SidebarRow::Folder(from_folder_id),
                            tab_pinned,
                            Some(SidebarRow::Tab(target_tab_index)),
                        );
                    }
                    self.propagate_folder_pinning(from_folder_id, tab_pinned);
                }
                DropTarget::UnpinnedSection => {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                        folder.pinned = false;
                        folder.parent_id = None;
                    }
                    self.propagate_folder_pinning(from_folder_id, false);
                    self.place_root_row_after(SidebarRow::Folder(from_folder_id), false, None);
                }
                DropTarget::RootAfter { pinned, row } => {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.id == from_folder_id) {
                        folder.pinned = pinned;
                        folder.parent_id = None;
                    }
                    self.propagate_folder_pinning(from_folder_id, pinned);
                    self.place_root_row_after(SidebarRow::Folder(from_folder_id), pinned, row);
                }
                _ => {}
            },
        }
        self.save_state();
        self.refresh();
    }

    fn calculate_drop_target(&self, x: i32, y: i32) -> DropTarget {
        if self.sidebar_width() <= 92
            || x < 0
            || (x as f32) >= self.sidebar_width
            || y >= self.workspace_switcher_bounds().top - 10
        {
            return DropTarget::None;
        }

        if point_in_rect(x, y, self.workspace_header_rect()) {
            return DropTarget::None;
        }
        for (_, rect) in self.workspace_switcher_items() {
            if point_in_rect(x, y, rect) {
                return DropTarget::None;
            }
        }
        if let Some(rect) = self.pinned_section_rect() {
            if point_in_rect(x, y, rect) {
                return DropTarget::PinnedSection;
            }
        }

        let rows = self.sidebar_row_rects();
        for (idx, (row, rect)) in rows.iter().enumerate() {
            if point_in_rect(x, y, *rect) {
                return match *row {
                    SidebarRow::Folder(folder_id) => {
                        let third = ((rect.bottom - rect.top) / 3).max(1);
                        let folder_pinned = self
                            .folders
                            .iter()
                            .find(|folder| folder.id == folder_id)
                            .map(|folder| folder.pinned)
                            .unwrap_or(false);
                        if y < rect.top + third {
                            if folder_pinned && previous_root_row(&rows, idx, true).is_none() {
                                DropTarget::PinnedSection
                            } else {
                                DropTarget::RootAfter {
                                    pinned: folder_pinned,
                                    row: previous_root_row(&rows, idx, folder_pinned),
                                }
                            }
                        } else if y > rect.bottom - third {
                            DropTarget::RootAfter {
                                pinned: folder_pinned,
                                row: Some(SidebarRow::Folder(folder_id)),
                            }
                        } else {
                            DropTarget::Folder(folder_id)
                        }
                    }
                    SidebarRow::Tab(index) => {
                        let tab_pinned =
                            self.tabs.get(index).map(|tab| tab.pinned).unwrap_or(false);
                        if y < (rect.top + rect.bottom) / 2 {
                            if tab_pinned && previous_root_row(&rows, idx, true).is_none() {
                                DropTarget::PinnedSection
                            } else {
                                DropTarget::RootAfter {
                                    pinned: tab_pinned,
                                    row: previous_root_row(&rows, idx, tab_pinned),
                                }
                            }
                        } else {
                            DropTarget::RootAfter {
                                pinned: tab_pinned,
                                row: Some(SidebarRow::Tab(index)),
                            }
                        }
                    }
                    SidebarRow::SplitGroup(group_id) => {
                        let tab_pinned = self
                            .split_group(group_id)
                            .and_then(|group| group.tab_ids.first().copied())
                            .and_then(|tab_id| self.tab_index_by_id(tab_id))
                            .and_then(|index| self.tabs.get(index))
                            .map(|tab| tab.pinned)
                            .unwrap_or(false);
                        if y < (rect.top + rect.bottom) / 2 {
                            if tab_pinned && previous_root_row(&rows, idx, true).is_none() {
                                DropTarget::PinnedSection
                            } else {
                                DropTarget::RootAfter {
                                    pinned: tab_pinned,
                                    row: previous_root_row(&rows, idx, tab_pinned),
                                }
                            }
                        } else {
                            DropTarget::RootAfter {
                                pinned: tab_pinned,
                                row: Some(SidebarRow::SplitGroup(group_id)),
                            }
                        }
                    }
                    SidebarRow::TabGhost(_) => DropTarget::None,
                    SidebarRow::Label(SidebarLabel::Tabs) => DropTarget::UnpinnedSection,
                    SidebarRow::Label(SidebarLabel::Pinned) => DropTarget::PinnedSection,
                };
            }
        }

        if let Some((_, divider)) = rows
            .iter()
            .find(|(row, _)| matches!(row, SidebarRow::Label(SidebarLabel::Tabs)))
        {
            if y < divider.top && y >= self.sidebar_rows_top() {
                DropTarget::PinnedSection
            } else if y >= divider.top {
                DropTarget::UnpinnedSection
            } else {
                DropTarget::None
            }
        } else {
            DropTarget::None
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
                        let favicon_left = 12;
                        let favicon = RECT {
                            left: favicon_left,
                            top: 11,
                            right: favicon_left + 18,
                            bottom: 29,
                        };
                        self.paint_tab_favicon(mem_dc, favicon, tab, false);
                        draw_text(
                            mem_dc,
                            &self.fonts.body,
                            &tab.title,
                            RECT {
                                left: 40,
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
                        draw_icon_glyph(
                            mem_dc,
                            &self.fonts.toolbar_icon,
                            glyph(0xE8B7).as_str(),
                            RECT {
                                left: 8,
                                top: 0,
                                right: 30,
                                bottom: ghost_height,
                            },
                            COLOR_MUTED,
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

            *self.drag_ghost.borrow_mut() = Some(DragGhost { handle: bitmap });

            CURRENT_DRAG_GHOST_BITMAP = Some(bitmap);

            let mut screen_pt = POINT {
                x: drag.current_x + 10,
                y: drag.current_y + 10,
            };
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
                Some(HINSTANCE(
                    LibraryLoader::GetModuleHandleW(None).unwrap_or_default().0,
                )),
                None,
            )
            .ok();

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
        if self.sidebar_mode == SidebarMode::Pushed
            || self.sidebar_expand_mode == SidebarMode::Pushed
            || self.topbar_mode == SidebarMode::Pushed
            || self.topbar_expand_mode == SidebarMode::Pushed
        {
            self.set_sidebar_mode(SidebarMode::Hidden);
            self.set_topbar_mode(SidebarMode::Hidden);
        } else if self.sidebar_mode == SidebarMode::Overlay
            || self.sidebar_expand_mode == SidebarMode::Overlay
            || self.topbar_mode == SidebarMode::Overlay
            || self.topbar_expand_mode == SidebarMode::Overlay
        {
            self.sidebar_expand_mode = SidebarMode::Pushed;
            self.topbar_expand_mode = SidebarMode::Pushed;
            if !self.animating_sidebar {
                self.sidebar_mode = SidebarMode::Pushed;
            }
            if !self.animating_topbar {
                self.topbar_mode = SidebarMode::Pushed;
            }
            self.layout();
            self.refresh();
        } else {
            self.sidebar_expand_mode = SidebarMode::Pushed;
            self.set_sidebar_mode(SidebarMode::Pushed);
            self.topbar_expand_mode = SidebarMode::Pushed;
            self.set_topbar_mode(SidebarMode::Pushed);
        }
    }

    fn set_sidebar_mode(&mut self, mode: SidebarMode) {
        if mode != SidebarMode::Hidden {
            if let Some(toast) = &mut self.download_toast {
                toast.fading = true;
            }
        }
        self.sidebar_target = match mode {
            SidebarMode::Hidden => SIDEBAR_HIDDEN,
            SidebarMode::Overlay | SidebarMode::Pushed => SIDEBAR_EXPANDED,
        };
        self.animating_sidebar = true;
        unsafe {
            if mode != SidebarMode::Hidden && self.topbar_mode != SidebarMode::Hidden {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
            }
            let _ = WindowsAndMessaging::SetTimer(Some(self.hwnd), SIDEBAR_TIMER_ID, 15, None);
        }
    }

    fn set_topbar_mode(&mut self, mode: SidebarMode) {
        self.topbar_target = match mode {
            SidebarMode::Hidden => TOPBAR_HIDDEN,
            SidebarMode::Overlay | SidebarMode::Pushed => TOPBAR_EXPANDED,
        };
        self.animating_topbar = true;
        unsafe {
            if mode != SidebarMode::Hidden && self.sidebar_mode != SidebarMode::Hidden {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
            }
            let _ = WindowsAndMessaging::SetTimer(Some(self.hwnd), TOPBAR_TIMER_ID, 15, None);
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
                self.mode_menu_open = false;
                self.settings_open = false;
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
        self.layout();
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn tick_topbar_animation(&mut self) {
        let distance = self.topbar_target - self.topbar_height;
        if distance.abs() < 0.75 {
            self.topbar_height = self.topbar_target;
            self.animating_topbar = false;
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), TOPBAR_TIMER_ID);
            }
            if self.topbar_height < 0.5 {
                self.topbar_mode = SidebarMode::Hidden;
                self.topbar_expand_mode = SidebarMode::Hidden;
                self.hovering_topbar = false;
                self.clear_webview_clipping();
                self.ensure_hover_detect_timer();
            } else if self.topbar_target >= TOPBAR_EXPANDED {
                self.topbar_mode = self.topbar_expand_mode;
                if self.topbar_mode == SidebarMode::Overlay {
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
            self.topbar_height += distance * 0.22;
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
        if self.sidebar_mode != SidebarMode::Overlay && self.topbar_mode != SidebarMode::Overlay {
            unsafe {
                let _ = WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
            }
            return;
        }
        if self.animating_sidebar || self.animating_topbar {
            return;
        }
        if self.drag_state.is_some() {
            return;
        }
        if self.overlay_menu.is_some() {
            return;
        }
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                if ScreenToClient(self.hwnd, &mut pt).as_bool() {
                    let sidebar_w = self.sidebar_width() as i32;
                    let topbar_h = self.topbar_height as i32;
                    let mut over_sidebar = false;
                    let mut over_topbar = false;
                    if self.sidebar_mode == SidebarMode::Overlay {
                        if pt.x <= sidebar_w + HOVER_ZONE && pt.x >= 0 && pt.y >= 0 && pt.y <= 10000
                        {
                            over_sidebar = true;
                        }
                    }
                    if self.topbar_mode == SidebarMode::Overlay {
                        if pt.y <= topbar_h + HOVER_ZONE && pt.y >= 0 && pt.x >= 0 && pt.x <= 10000
                        {
                            over_topbar = true;
                        }
                    }

                    if !over_sidebar && self.sidebar_mode == SidebarMode::Overlay {
                        self.settings_open = false;
                        self.mode_menu_open = false;
                        self.sidebar_expand_mode = SidebarMode::Hidden;
                        self.set_sidebar_mode(SidebarMode::Hidden);
                    }
                    if !over_topbar && self.topbar_mode == SidebarMode::Overlay {
                        self.topbar_expand_mode = SidebarMode::Hidden;
                        self.set_topbar_mode(SidebarMode::Hidden);
                    }

                    if self.sidebar_mode != SidebarMode::Overlay
                        && self.topbar_mode != SidebarMode::Overlay
                    {
                        let _ =
                            WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_LEAVE_TIMER_ID);
                    }
                }
            }
        }
    }

    fn check_hover_detect(&mut self) {
        if (self.sidebar_mode != SidebarMode::Hidden || self.animating_sidebar)
            && (self.topbar_mode != SidebarMode::Hidden || self.animating_topbar)
        {
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
                        if self.sidebar_mode == SidebarMode::Hidden && !self.animating_sidebar {
                            self.sidebar_expand_mode = SidebarMode::Overlay;
                            self.hovering_sidebar = true;
                            self.set_sidebar_mode(SidebarMode::Overlay);
                        }
                    }
                    if pt.y < HOVER_ZONE + 8 && pt.y >= 0 && pt.x >= 0 {
                        if self.topbar_mode == SidebarMode::Hidden && !self.animating_topbar {
                            self.topbar_expand_mode = SidebarMode::Overlay;
                            self.hovering_topbar = true;
                            self.set_topbar_mode(SidebarMode::Overlay);
                        }
                    }

                    if self.sidebar_mode != SidebarMode::Hidden
                        && self.topbar_mode != SidebarMode::Hidden
                    {
                        let _ =
                            WindowsAndMessaging::KillTimer(Some(self.hwnd), HOVER_DETECT_TIMER_ID);
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

    fn set_fullscreen_state(&mut self, enable: bool) {
        if self.fullscreen == enable {
            return;
        }
        unsafe {
            if enable {
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

    fn toggle_fullscreen(&mut self) {
        let next = !self.fullscreen;
        self.set_fullscreen_state(next);
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

    unsafe {
        if let Ok(existing_hwnd) = WindowsAndMessaging::FindWindowW(CLASS_NAME, None) {
            if !existing_hwnd.is_invalid() {
                let args: Vec<String> = std::env::args().collect();
                let mut startup_url = String::new();
                if args.len() > 1 {
                    for arg in args.iter().skip(1) {
                        if !arg.starts_with('-') && !arg.starts_with('/') {
                            startup_url = normalize_address(arg);
                            break;
                        }
                    }
                }

                let _ =
                    WindowsAndMessaging::ShowWindow(existing_hwnd, WindowsAndMessaging::SW_RESTORE);
                let _ = WindowsAndMessaging::SetForegroundWindow(existing_hwnd);

                if !startup_url.is_empty() {
                    let bytes = startup_url.as_bytes();
                    let cds = COPYDATASTRUCT {
                        dwData: 0x1234,
                        cbData: bytes.len() as u32,
                        lpData: bytes.as_ptr() as *const _ as *mut _,
                    };
                    let _ = WindowsAndMessaging::SendMessageW(
                        existing_hwnd,
                        WM_COPYDATA,
                        None,
                        Some(LPARAM(&cds as *const _ as isize)),
                    );
                }
                return Ok(());
            }
        }
    }

    set_process_dpi_awareness();
    register_window_class()?;
    let hwnd = create_main_window()?;
    let environment = create_environment()?;

    let app = Box::new(App::new(hwnd, environment)?);
    unsafe {
        SetWindowLong(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        with_app(hwnd, |app| {
            app.request_missing_favicons();
            app.refresh_extensions();
        });
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
            let options = CoreWebView2EnvironmentOptions::default();
            options.set_are_browser_extensions_enabled(true);
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                *user_data.as_ref().as_pcwstr(),
                &ICoreWebView2EnvironmentOptions::from(options),
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

fn create_find_edit(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_TABSTOP.0),
            0,
            0,
            220,
            22,
            Some(parent),
            Some(HMENU(FIND_ID as usize as *mut _)),
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        let _ = WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_SETMARGINS,
            Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
            Some(LPARAM((2 | (2 << 16)) as isize)),
        );
        set_edit_cue_banner(hwnd, "Find in page");
        OLD_FIND_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            find_edit_proc as *const () as isize,
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
            WINDOW_STYLE(
                WS_CHILD.0 | WS_CLIPSIBLINGS.0 | 0x00000100, /* SS_NOTIFY */
            ),
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
            WINDOW_STYLE(
                WS_CHILD.0 | WS_CLIPSIBLINGS.0 | 0x00000100, /* SS_NOTIFY */
            ),
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

fn create_download_popup(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(DOWNLOAD_POPUP_ID as usize as *mut _)),
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        OLD_DOWNLOAD_POPUP_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            download_popup_proc as *const () as isize,
        ));
        Ok(hwnd)
    }
}

fn create_bookmark_popup(parent: HWND) -> AppResult<HWND> {
    unsafe {
        let hwnd = WindowsAndMessaging::CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(BOOKMARK_POPUP_ID as usize as *mut _)),
            Some(HINSTANCE(LibraryLoader::GetModuleHandleW(None)?.0)),
            None,
        )?;
        OLD_BOOKMARK_POPUP_PROC = mem::transmute(WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            bookmark_popup_proc as *const () as isize,
        ));
        let _ = WindowsAndMessaging::ShowWindow(hwnd, WindowsAndMessaging::SW_HIDE);
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
                    let _ = WindowsAndMessaging::ShowWindow(
                        app.overlay_menu_hwnd,
                        WindowsAndMessaging::SW_HIDE,
                    );
                    app.refresh();
                });
            }
            LRESULT(0)
        }
        _ => {
            WindowsAndMessaging::CallWindowProcW(OLD_OVERLAY_MENU_PROC, hwnd, msg, w_param, l_param)
        }
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
        _ => WindowsAndMessaging::CallWindowProcW(OLD_DRAG_GHOST_PROC, hwnd, msg, w_param, l_param),
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

unsafe extern "system" fn find_edit_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        let key = w_param.0 as u32;
        if key == VK_RETURN.0 as u32 {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                with_app(parent, |app| {
                    app.run_find_script(if shift { -1 } else { 1 })
                });
            }
            return LRESULT(0);
        }
        if key == VK_ESCAPE.0 as u32 {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| app.close_find_bar());
            }
            return LRESULT(0);
        }
        if key == 0x46 && (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
            if let Ok(parent) = WindowsAndMessaging::GetParent(hwnd) {
                with_app(parent, |app| app.hide_find_bar());
            }
            return LRESULT(0);
        }
    }
    if msg == WM_CHAR {
        let ch = w_param.0 as u32;
        if ch == VK_RETURN.0 as u32 {
            return LRESULT(0);
        }
        if ch < 0x20 && !matches!(ch, 0x08 | 0x09 | 0x0D | 0x1B) {
            return LRESULT(0);
        }
    }
    WindowsAndMessaging::CallWindowProcW(OLD_FIND_PROC, hwnd, msg, w_param, l_param)
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
        if key == 0x09 {
            // VK_TAB
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
        if key == 0x26 || key == 0x28 {
            // VK_UP or VK_DOWN
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
                        } else if next >= app.command_scroll_offset + 6 {
                            app.command_scroll_offset = next - 5;
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
                if key == 8 || key == 46 {
                    // VK_BACK or VK_DELETE
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
                if total > 6 {
                    if delta < 0 {
                        app.command_scroll_offset = (app.command_scroll_offset + 1).min(total - 6);
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
                    for (row_index, (tab_index, _title, url)) in app
                        .command_suggestions()
                        .into_iter()
                        .skip(app.command_scroll_offset)
                        .take(6)
                        .enumerate()
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
                    if total > 6 {
                        if delta < 0 {
                            app.command_scroll_offset =
                                (app.command_scroll_offset + 1).min(total - 6);
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

unsafe extern "system" fn download_popup_proc(
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
                    if let Some(toast) = &app.download_toast {
                        let elapsed = toast.start_time.elapsed().as_millis();
                        if elapsed < 3000 || toast.fading {
                            let rect = client_rect(hwnd);
                            draw_download_popup_gdi(hdc, rect, elapsed as u64);
                        }
                    }
                });
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => WindowsAndMessaging::CallWindowProcW(
            OLD_DOWNLOAD_POPUP_PROC,
            hwnd,
            msg,
            w_param,
            l_param,
        ),
    }
}

unsafe extern "system" fn bookmark_popup_proc(
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
                    if let Some(toast) = &app.bookmark_toast {
                        let rect = client_rect(hwnd);
                        draw_bookmark_popup_gdi(
                            hdc,
                            rect,
                            toast.start_time.elapsed().as_millis() as u64,
                            toast.is_unbookmark,
                        );
                    }
                });
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => WindowsAndMessaging::CallWindowProcW(
            OLD_BOOKMARK_POPUP_PROC,
            hwnd,
            msg,
            w_param,
            l_param,
        ),
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    match msg {
        msg if msg == FAVICON_FETCHED_MSG => {
            let ptr = w_param.0 as *mut FaviconFetchResult;
            if !ptr.is_null() {
                unsafe {
                    let result = *Box::from_raw(ptr);
                    with_app(hwnd, |app| app.complete_favicon_fetch(result));
                }
            }
            LRESULT(0)
        }
        WM_COPYDATA => {
            unsafe {
                let cds = &*(l_param.0 as *const COPYDATASTRUCT);
                if cds.dwData == 0x1234 {
                    let len = cds.cbData as usize;
                    let ptr = cds.lpData as *const u8;
                    let slice = std::slice::from_raw_parts(ptr, len);
                    if let Ok(url_str) = std::str::from_utf8(slice) {
                        let url = url_str.to_string();
                        with_app(hwnd, move |app| {
                            let _ = app.create_tab(&url);
                            if let Some(index) = app.tabs.iter().position(|t| t.url == url) {
                                app.switch_to(index, true);
                            }
                            app.refresh();
                        });
                        let _ =
                            WindowsAndMessaging::ShowWindow(hwnd, WindowsAndMessaging::SW_RESTORE);
                        let _ = WindowsAndMessaging::SetForegroundWindow(hwnd);
                    }
                }
            }
            LRESULT(1)
        }
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
                            let params =
                                &mut *(l_param.0 as *mut WindowsAndMessaging::NCCALCSIZE_PARAMS);
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
                    let extensions = app.extension_button_rect();
                    let address = app.address_pill_rect();
                    let find_bar = app.find_bar_rect();
                    let (min_btn, max_btn, close_btn) = app.window_button_rects();
                    let split_layout = app.split_layout_button_rect();
                    let split_unsplit = app.split_unsplit_button_rect();

                    if point_in_rect(pt.x, pt.y, logo)
                        || point_in_rect(pt.x, pt.y, new_tab)
                        || point_in_rect(pt.x, pt.y, back)
                        || point_in_rect(pt.x, pt.y, forward)
                        || point_in_rect(pt.x, pt.y, reload)
                        || point_in_rect(pt.x, pt.y, extensions)
                        || point_in_rect(pt.x, pt.y, address)
                        || split_layout
                            .map(|rect| point_in_rect(pt.x, pt.y, rect))
                            .unwrap_or(false)
                        || split_unsplit
                            .map(|rect| point_in_rect(pt.x, pt.y, rect))
                            .unwrap_or(false)
                        || (app.find_open && point_in_rect(pt.x, pt.y, find_bar))
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
                if width > 0 && height > 0 {
                    with_app(hwnd, |app| {
                        let mem_dc = {
                            let mut cache = app.paint_cache.borrow_mut();
                            let cached = cache.get_or_insert_with(|| {
                                let dc = CreateCompatibleDC(Some(hdc));
                                let bitmap = CreateCompatibleBitmap(hdc, width, height);
                                let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
                                PaintCache {
                                    bitmap,
                                    dc,
                                    width,
                                    height,
                                    old_bitmap,
                                }
                            });
                            if cached.width != width || cached.height != height {
                                let _ = SelectObject(cached.dc, cached.old_bitmap);
                                let _ = DeleteObject(HGDIOBJ(cached.bitmap.0));
                                let _ = DeleteDC(cached.dc);
                                let dc = CreateCompatibleDC(Some(hdc));
                                let bitmap = CreateCompatibleBitmap(hdc, width, height);
                                let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
                                *cached = PaintCache {
                                    bitmap,
                                    dc,
                                    width,
                                    height,
                                    old_bitmap,
                                };
                            }
                            cached.dc
                        };
                        fill_rect(mem_dc, rect, app.dominant_color);
                        app.paint(mem_dc);
                        let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY);
                    });
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| {
                if !app.start_drag_candidate(x, y) {
                    app.handle_click(x, y);
                }
            });
            LRESULT(0)
        }
        WindowsAndMessaging::WM_LBUTTONDBLCLK => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| {
                if !app.start_drag_candidate(x, y) {
                    app.handle_click(x, y);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = loword(l_param.0 as u32) as i16 as i32;
            let y = hiword(l_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| {
                let pending_click = app
                    .drag_state
                    .as_ref()
                    .filter(|drag| !drag.active)
                    .map(|drag| (drag.start_x, drag.start_y));
                if !app.finish_drag(x, y) {
                    if let Some((click_x, click_y)) = pending_click {
                        app.handle_click(click_x, click_y);
                    }
                }
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
                    if delta < 0 {
                        let total_rows = app.sidebar_rows().len();
                        let max_offset = total_rows.saturating_sub(1);
                        app.sidebar_scroll_offset = (app.sidebar_scroll_offset + 1).min(max_offset);
                    } else {
                        app.sidebar_scroll_offset = app.sidebar_scroll_offset.saturating_sub(1);
                    }
                    unsafe {
                        let _ = InvalidateRect(Some(app.hwnd), None, false);
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSEHWHEEL => {
            let delta = hiword(w_param.0 as u32) as i16 as i32;
            with_app(hwnd, |app| {
                let now = std::time::Instant::now();
                if let Some(last) = app.last_workspace_swipe {
                    if now.duration_since(last).as_millis() > 300 {
                        app.workspace_swipe_accum = 0;
                    }
                }
                app.last_workspace_swipe = Some(now);
                app.workspace_swipe_accum += delta;

                if app.workspace_swipe_accum > 150 {
                    app.switch_workspace_by_delta(1);
                    app.workspace_swipe_accum = 0;
                } else if app.workspace_swipe_accum < -150 {
                    app.switch_workspace_by_delta(-1);
                    app.workspace_swipe_accum = 0;
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
            if w_param.0 == TOPBAR_TIMER_ID {
                with_app(hwnd, |app| app.tick_topbar_animation());
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
            if w_param.0 == LOADING_TIMER_ID {
                with_app(hwnd, |app| unsafe {
                    let _ = InvalidateRect(Some(app.hwnd), None, false);
                });
                return LRESULT(0);
            }
            if w_param.0 == DOWNLOAD_TIMER_ID {
                with_app(hwnd, |app| {
                    app.tick_download_toast();
                    app.tick_bookmark_toast();
                    app.tick_download_removal();
                    app.tick_download_panel_animation();
                    app.poll_downloads();
                    unsafe {
                        let _ = InvalidateRect(Some(app.hwnd), None, false);
                        if app.download_popup_hwnd != HWND(std::ptr::null_mut()) {
                            let _ = InvalidateRect(Some(app.download_popup_hwnd), None, false);
                        }
                        if app.bookmark_popup_hwnd != HWND(std::ptr::null_mut()) {
                            let _ = InvalidateRect(Some(app.bookmark_popup_hwnd), None, false);
                        }
                    }
                });
                return LRESULT(0);
            }

            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_COMMAND => {
            let id = loword(w_param.0 as u32) as i32;
            let code = hiword(w_param.0 as u32) as u16;
            if id == FIND_ID {
                if code == 0x0300 {
                    with_app(hwnd, |app| {
                        app.find_query = get_window_text(app.find_hwnd);
                        app.run_find_script(0);
                    });
                }
                return LRESULT(0);
            }
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
                                app.request_command_favicons();
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
                    } else if key == 0x25
                        || key == 0x26
                        || key == 0x27
                        || key == 0x28
                        || key == 0x24
                        || key == 0x23
                    {
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
        WindowsAndMessaging::WM_ACTIVATE => {
            with_app(hwnd, |app| {
                let is_default = is_aster_default_browser();
                if is_default {
                    if app.show_default_bubble {
                        app.show_default_bubble = false;
                        app.set_sidebar_mode(SidebarMode::Hidden);
                        app.refresh();
                    }
                } else if !app.default_bubble_dismissed && !app.show_default_bubble {
                    app.show_default_bubble = true;
                    app.refresh();
                }
                if let Some(index) = app.tabs.iter().position(|t| t.url == "aster:settings") {
                    app.load_settings_page(index);
                }
                if let Some(index) = app.tabs.iter().position(|t| t.url == "aster:history") {
                    app.load_history_page(index);
                }
                if let Some(index) = app.tabs.iter().position(|t| t.url == "aster:extensions") {
                    app.load_extensions_page(index);
                    app.refresh_extensions();
                }
            });
            unsafe { WindowsAndMessaging::DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_SETFOCUS => {
            with_app(hwnd, |app| {
                let is_default = is_aster_default_browser();
                if is_default {
                    if app.show_default_bubble {
                        app.show_default_bubble = false;
                        app.set_sidebar_mode(SidebarMode::Hidden);
                        app.refresh();
                    }
                } else if !app.default_bubble_dismissed && !app.show_default_bubble {
                    app.show_default_bubble = true;
                    app.refresh();
                }
                if let Some(index) = app.tabs.iter().position(|t| t.url == "aster:settings") {
                    app.load_settings_page(index);
                }
                if let Some(index) = app.tabs.iter().position(|t| t.url == "aster:history") {
                    app.load_history_page(index);
                }
                if let Some(index) = app.tabs.iter().position(|t| t.url == "aster:extensions") {
                    app.load_extensions_page(index);
                    app.refresh_extensions();
                }

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
        msg if msg == FOCUS_EDIT_MSG => unsafe {
            let edit = HWND(w_param.0 as *mut _);
            let _ = SetFocus(Some(edit));
            LRESULT(0)
        },
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
        let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
        with_app(hwnd, |app| match key {
            _ if app.run_custom_keybind(key, ctrl, alt, shift) => {}
            0x5A if ctrl && shift => {
                app.reopen_closed_tab();
            }
            0x44 if ctrl => {
                app.toggle_active_bookmark();
            }
            0x46 if ctrl => {
                if app.find_open {
                    app.hide_find_bar();
                } else {
                    app.open_find_bar();
                }
            }
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
            0x52 if ctrl => {
                app.reload();
            }
            0x30 | 0x60 if ctrl => app.reset_active_zoom(),
            0xBB | 0x6B if ctrl => app.adjust_active_zoom(0.1),
            0xBD | 0x6D if ctrl => app.adjust_active_zoom(-0.1),
            0x25 if alt => app.go_back(),
            0x27 if alt => app.go_forward(),
            0x41 if alt => app.go_back(),
            0x44 if alt => app.go_forward(),
            0x57 if alt => {
                app.switch_tab_above();
            }
            0x53 if alt => {
                app.switch_tab_below();
            }
            0x48 if ctrl && alt => {
                if app.active_split_group_id().is_some() {
                    app.set_active_split_layout(SplitLayout::Horizontal);
                } else if let Some(index) = app.active_tab_index() {
                    app.split_tab_with_active(index, SplitLayout::Horizontal);
                }
            }
            0x56 if ctrl && alt => {
                if app.active_split_group_id().is_some() {
                    app.set_active_split_layout(SplitLayout::Vertical);
                } else if let Some(index) = app.active_tab_index() {
                    app.split_tab_with_active(index, SplitLayout::Vertical);
                }
            }
            0x47 if ctrl && alt => {
                if app.active_split_group_id().is_some() {
                    app.set_active_split_layout(SplitLayout::Grid);
                } else if let Some(index) = app.active_tab_index() {
                    app.split_tab_with_active(index, SplitLayout::Grid);
                }
            }
            0x55 if ctrl && alt => {
                if let Some(group_id) = app.active_split_group_id() {
                    app.unsplit_group(group_id);
                }
            }
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
        let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
        matches!(key, 0x44 | 0x46 | 0x4C | 0x53 | 0x54 | 0x57 | 0x52 | 0x30 | 0x60 | 0xBB | 0x6B | 0xBD | 0x6D if ctrl)
            || (key == 0x5A && ctrl && shift)
            || matches!(key, 0x25 | 0x27 | 0x41 | 0x44 | 0x57 | 0x53 if alt)
            || matches!(key, 0x47 | 0x48 | 0x55 | 0x56 if ctrl && alt)
            || key == VK_F5.0 as u32
            || key == VK_F11.0 as u32
    }
}

fn default_action_for_event(key: u32, ctrl: bool, alt: bool, shift: bool) -> Option<&'static str> {
    match key {
        0x4C if ctrl => Some("Navigate"),
        0x44 if ctrl => Some("Bookmark site"),
        0x46 if ctrl => Some("Find in page"),
        0x54 if ctrl => Some("New tab"),
        0x57 if ctrl => Some("Close tab"),
        0x52 if ctrl => Some("Reload"),
        0x30 | 0x60 if ctrl => Some("Reset zoom"),
        0xBB | 0x6B if ctrl => Some("Zoom in"),
        0xBD | 0x6D if ctrl => Some("Zoom out"),
        0x5A if ctrl && shift => Some("Reopen closed tab"),
        0x53 if ctrl => Some("Toggle sidebar"),
        0x25 | 0x41 if alt => Some("Go back"),
        0x27 | 0x44 if alt => Some("Go forward"),
        0x57 if alt => Some("Switch tab above"),
        0x53 if alt => Some("Switch tab below"),
        0x48 if ctrl && alt => Some("Split horizontal"),
        0x56 if ctrl && alt => Some("Split vertical"),
        0x47 if ctrl && alt => Some("Split grid"),
        0x55 if ctrl && alt => Some("Unsplit tabs"),
        code if code == VK_F5.0 as u32 => Some("Reload"),
        code if code == VK_F11.0 as u32 => Some("Toggle fullscreen"),
        _ => None,
    }
}

fn combo_label_for_event(key: u32, ctrl: bool, alt: bool, shift: bool) -> String {
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    let key_label = match key {
        0x30..=0x39 => char::from_u32(key).map(|ch| ch.to_string()),
        0x41..=0x5A => char::from_u32(key).map(|ch| ch.to_string()),
        0x60..=0x69 => char::from_u32(key - 0x30).map(|ch| ch.to_string()),
        0xBB | 0x6B => Some("+".to_string()),
        0xBD | 0x6D => Some("-".to_string()),
        0x25 => Some("ArrowLeft".to_string()),
        0x27 => Some("ArrowRight".to_string()),
        code if code == VK_F5.0 as u32 => Some("F5".to_string()),
        code if code == VK_F11.0 as u32 => Some("F11".to_string()),
        _ => None,
    };
    if let Some(label) = key_label {
        parts.push(label);
    }
    if parts.is_empty() {
        String::new()
    } else {
        parts.join("+")
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
    Extensions,
}

fn draw_toolbar_icon_button(
    hdc: HDC,
    rect: RECT,
    icon: IconKind,
    hovered: bool,
    icon_font: &HFONT,
) {
    unsafe {
        if hovered {
            fill_round_rect(hdc, rect, COLOR_SURFACE_HOVER, 8);
        }
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
            if hovered { COLOR_TEXT } else { COLOR_MUTED },
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

fn draw_settings_button(hdc: HDC, rect: RECT, hovered: bool) {
    unsafe {
        if hovered {
            fill_round_rect(hdc, rect, COLOR_SURFACE_HOVER, 10);
        }
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let r = 2;
        let spacing = 6;
        BRUSH_CACHE.with(|cache| {
            let mut c = cache.borrow_mut();
            let brush = *c
                .brushes
                .entry(COLOR_MUTED)
                .or_insert_with(|| solid_brush(COLOR_MUTED));
            let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
            let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
            let size = r * 2;
            for dy in [-spacing, 0, spacing] {
                let _ = RoundRect(
                    hdc,
                    cx - r,
                    cy + dy - r,
                    cx - r + size,
                    cy + dy - r + size,
                    size,
                    size,
                );
            }
            let _ = SelectObject(hdc, old_pen);
            let _ = SelectObject(hdc, old_brush);
        });
    }
}

unsafe fn draw_download_indicator(
    hdc: HDC,
    rect: RECT,
    progress: f32,
    completed: bool,
    completed_at: Option<std::time::Instant>,
    cancelled: bool,
    cancelled_at: Option<std::time::Instant>,
    hovered: bool,
) {
    let size = (rect.right - rect.left).min(rect.bottom - rect.top).max(1);

    let (morph, is_cancelled) = if cancelled {
        let m = cancelled_at
            .map(|at| (at.elapsed().as_millis() as f32 / 420.0).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        (m, true)
    } else {
        let m = completed_at
            .map(|at| (at.elapsed().as_millis() as f32 / 420.0).clamp(0.0, 1.0))
            .unwrap_or(if completed { 1.0 } else { 0.0 });
        (m, false)
    };
    let pixels = render_download_indicator_pixels(size, progress, morph, is_cancelled, hovered);
    if let Some(bitmap) = create_bgra_bitmap(size, size, &pixels) {
        let mem_dc = CreateCompatibleDC(Some(hdc));
        if !mem_dc.is_invalid() {
            let old = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = AlphaBlend(
                hdc, rect.left, rect.top, size, size, mem_dc, 0, 0, size, size, blend,
            );
            let _ = SelectObject(mem_dc, old);
            let _ = DeleteDC(mem_dc);
        }
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
}

unsafe fn draw_download_popup_gdi(hdc: HDC, rect: RECT, elapsed_ms: u64) {
    let size = (rect.right - rect.left).min(rect.bottom - rect.top);
    if size <= 0 {
        return;
    }
    let radius = size / 2;

    fill_round_rect(hdc, rect, COLOR_PANEL_2, radius);

    let cx = rect.left + radius;
    let cy = rect.top + radius;
    let ring_r = (size as f32 * 0.38) as i32;
    let ring_w = 3i32;

    let t = (elapsed_ms % 1200) as f32 / 1200.0;
    let rotation = t * std::f32::consts::TAU;

    let sweep_start = rotation;
    let sweep_end = sweep_start + std::f32::consts::TAU * 0.75;
    let steps = 36;
    for i in 0..steps {
        let angle = sweep_start + (i as f32 / steps as f32) * (sweep_end - sweep_start);
        let x = cx + (ring_r as f32 * angle.cos()) as i32;
        let y = cy + (ring_r as f32 * angle.sin()) as i32;
        fill_round_rect(
            hdc,
            RECT {
                left: x - ring_w / 2,
                top: y - ring_w / 2,
                right: x + ring_w / 2 + 1,
                bottom: y + ring_w / 2 + 1,
            },
            0xf16f63,
            ring_w / 2,
        );
    }

    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    let stroke = size as f32 * 0.065;
    draw_aa_line(
        &mut pixels,
        size,
        center,
        size as f32 * 0.27,
        center,
        size as f32 * 0.62,
        stroke,
        COLOR_MUTED,
        1.0,
    );
    draw_aa_line(
        &mut pixels,
        size,
        size as f32 * 0.36,
        size as f32 * 0.50,
        center,
        size as f32 * 0.64,
        stroke,
        COLOR_MUTED,
        1.0,
    );
    draw_aa_line(
        &mut pixels,
        size,
        size as f32 * 0.64,
        size as f32 * 0.50,
        center,
        size as f32 * 0.64,
        stroke,
        COLOR_MUTED,
        1.0,
    );
    draw_aa_line(
        &mut pixels,
        size,
        size as f32 * 0.34,
        size as f32 * 0.72,
        size as f32 * 0.66,
        size as f32 * 0.72,
        stroke,
        COLOR_MUTED,
        0.75,
    );
    if let Some(bitmap) = create_bgra_bitmap(size, size, &pixels) {
        let mem_dc = CreateCompatibleDC(Some(hdc));
        if !mem_dc.is_invalid() {
            let old = SelectObject(mem_dc, HGDIOBJ(bitmap.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = AlphaBlend(
                hdc, rect.left, rect.top, size, size, mem_dc, 0, 0, size, size, blend,
            );
            let _ = SelectObject(mem_dc, old);
            let _ = DeleteDC(mem_dc);
        }
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
}

unsafe fn draw_bookmark_popup_gdi(hdc: HDC, rect: RECT, elapsed_ms: u64, is_unbookmark: bool) {
    let slide = if elapsed_ms < 180 {
        1.0 - (elapsed_ms as f32 / 180.0)
    } else {
        0.0
    };
    let mut body = rect;
    body.top += (slide * 26.0) as i32;
    fill_round_rect(hdc, body, 0x111111, 12);
    draw_outline(hdc, body, 0x343434, 12);
    let star_rect = RECT {
        left: body.left + 14,
        top: body.top + 10,
        right: body.left + 42,
        bottom: body.top + 38,
    };
    let icon_font = create_font_with_face(20, 600, w!("Segoe UI Symbol"))
        .unwrap_or(HFONT(std::ptr::null_mut()));
    draw_centered_text(
        hdc,
        &icon_font,
        if is_unbookmark { "☆" } else { "*" },
        star_rect,
        if is_unbookmark { 0x888888 } else { 0x27d8ff },
    );
    if icon_font != HFONT(std::ptr::null_mut()) {
        let _ = DeleteObject(HGDIOBJ(icon_font.0));
    }
    let text_font = create_font(14, 600).unwrap_or(HFONT(std::ptr::null_mut()));
    draw_text(
        hdc,
        &text_font,
        if is_unbookmark {
            "Unbookmarked"
        } else {
            "Bookmarked Site!"
        },
        RECT {
            left: body.left + 52,
            top: body.top,
            right: body.right - 12,
            bottom: body.bottom,
        },
        COLOR_TEXT,
    );
    if text_font != HFONT(std::ptr::null_mut()) {
        let _ = DeleteObject(HGDIOBJ(text_font.0));
    }
}

unsafe fn draw_download_toast_gdi(hdc: HDC, rect: RECT, _elapsed_ms: u64, _alpha: f32) {
    let size = (rect.right - rect.left).min(rect.bottom - rect.top);
    if size <= 0 {
        return;
    }
    let radius = size / 2;

    fill_round_rect(hdc, rect, COLOR_PANEL_2, radius);

    let cx = rect.left + radius;
    let cy = rect.top + radius;
    let ring_r = (size as f32 * 0.38) as i32;
    let ring_w = 3i32;

    let t = (_elapsed_ms % 1200) as f32 / 1200.0;
    let rotation = t * std::f32::consts::TAU;

    let sweep_start = rotation;
    let sweep_end = sweep_start + std::f32::consts::TAU * 0.75;
    let steps = 36;
    for i in 0..steps {
        let angle = sweep_start + (i as f32 / steps as f32) * (sweep_end - sweep_start);
        let x = cx + (ring_r as f32 * angle.cos()) as i32;
        let y = cy + (ring_r as f32 * angle.sin()) as i32;
        fill_round_rect(
            hdc,
            RECT {
                left: x - ring_w / 2,
                top: y - ring_w / 2,
                right: x + ring_w / 2 + 1,
                bottom: y + ring_w / 2 + 1,
            },
            0xf16f63,
            ring_w / 2,
        );
    }

    let half_i = (size as f32 * 0.06) as i32;
    let rt = rect.top;
    let arrow_top = rt + (size as f32 * 0.28) as i32;
    let arrow_mid = rt + (size as f32 * 0.50) as i32;
    let arrow_bot = rt + (size as f32 * 0.62) as i32;
    let arrow_wid = (size as f32 * 0.16) as i32;
    let arrow_base = rt + (size as f32 * 0.72) as i32;
    let arrow_color = COLOR_MUTED;

    fill_rect(
        hdc,
        RECT {
            left: cx - half_i,
            top: arrow_top,
            right: cx + half_i + 1,
            bottom: arrow_bot,
        },
        arrow_color,
    );
    fill_rect(
        hdc,
        RECT {
            left: cx - arrow_wid - half_i,
            top: arrow_mid - half_i,
            right: cx + half_i + 1,
            bottom: arrow_mid + half_i + 1,
        },
        arrow_color,
    );
    fill_rect(
        hdc,
        RECT {
            left: cx - half_i,
            top: arrow_mid - half_i,
            right: cx + arrow_wid + half_i + 1,
            bottom: arrow_mid + half_i + 1,
        },
        arrow_color,
    );
    fill_rect(
        hdc,
        RECT {
            left: cx - arrow_wid,
            top: arrow_base - half_i,
            right: cx + arrow_wid + 1,
            bottom: arrow_base + half_i + 1,
        },
        arrow_color,
    );
}

fn render_download_indicator_pixels(
    size: i32,
    progress: f32,
    morph: f32,
    cancelled: bool,
    hovered: bool,
) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    let radius = size as f32 * 0.43;
    let x_color = 0x3333FF;
    let bg = if hovered {
        mix_color(COLOR_PANEL_2, COLOR_SURFACE_HOVER, 0.76)
    } else {
        COLOR_PANEL_2
    };
    let morph_amount = if cancelled {
        morph.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let circle_bg = if cancelled {
        mix_color(bg, x_color, 0.15 * morph_amount)
    } else {
        bg
    };
    draw_aa_filled_circle(&mut pixels, size, center, center, radius, circle_bg, 1.0);
    draw_aa_ring(
        &mut pixels,
        size,
        center,
        center,
        radius - 0.7,
        1.35,
        if cancelled {
            mix_color(0x565656, x_color, morph_amount)
        } else {
            0x565656
        },
        1.0,
    );
    if !cancelled {
        draw_aa_arc(
            &mut pixels,
            size,
            center,
            center,
            radius - 0.7,
            1.8,
            progress.clamp(0.0, 1.0),
            COLOR_ACCENT,
            1.0,
            0.0,
        );
    }

    let download_alpha = (1.0 - morph).clamp(0.0, 1.0);
    let icon_alpha = morph.clamp(0.0, 1.0);
    if download_alpha > 0.02 {
        let color = COLOR_MUTED;
        let stroke = size as f32 * 0.065;
        draw_aa_line(
            &mut pixels,
            size,
            center,
            size as f32 * 0.27,
            center,
            size as f32 * 0.62,
            stroke,
            color,
            download_alpha,
        );
        draw_aa_line(
            &mut pixels,
            size,
            size as f32 * 0.36,
            size as f32 * 0.50,
            center,
            size as f32 * 0.64,
            stroke,
            color,
            download_alpha,
        );
        draw_aa_line(
            &mut pixels,
            size,
            size as f32 * 0.64,
            size as f32 * 0.50,
            center,
            size as f32 * 0.64,
            stroke,
            color,
            download_alpha,
        );
        draw_aa_line(
            &mut pixels,
            size,
            size as f32 * 0.34,
            size as f32 * 0.72,
            size as f32 * 0.66,
            size as f32 * 0.72,
            stroke,
            color,
            download_alpha * 0.75,
        );
    }

    if icon_alpha > 0.02 {
        if cancelled {
            let stroke = size as f32 * 0.065;
            draw_aa_line(
                &mut pixels,
                size,
                size as f32 * 0.33,
                size as f32 * 0.33,
                size as f32 * 0.67,
                size as f32 * 0.67,
                stroke,
                x_color,
                icon_alpha,
            );
            draw_aa_line(
                &mut pixels,
                size,
                size as f32 * 0.67,
                size as f32 * 0.33,
                size as f32 * 0.33,
                size as f32 * 0.67,
                stroke,
                x_color,
                icon_alpha,
            );
        } else {
            let stroke = size as f32 * 0.058;
            draw_aa_line(
                &mut pixels,
                size,
                size as f32 * 0.33,
                size as f32 * 0.53,
                size as f32 * 0.45,
                size as f32 * 0.64,
                stroke,
                COLOR_MUTED,
                icon_alpha,
            );
            draw_aa_line(
                &mut pixels,
                size,
                size as f32 * 0.45,
                size as f32 * 0.64,
                size as f32 * 0.69,
                size as f32 * 0.38,
                stroke,
                COLOR_MUTED,
                icon_alpha,
            );
        }
    }
    pixels
}

fn draw_aa_filled_circle(
    pixels: &mut [u8],
    size: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    color: u32,
    alpha: f32,
) {
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.5 - dist).clamp(0.0, 1.0) * alpha;
            if coverage > 0.0 {
                blend_pixel(pixels, size, x, y, color, coverage);
            }
        }
    }
}

fn draw_aa_ring(
    pixels: &mut [u8],
    size: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    width: f32,
    color: u32,
    alpha: f32,
) {
    let half = width / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (half + 0.7 - (dist - radius).abs()).clamp(0.0, 1.0) * alpha;
            if coverage > 0.0 {
                blend_pixel(pixels, size, x, y, color, coverage);
            }
        }
    }
}

fn draw_aa_arc(
    pixels: &mut [u8],
    size: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    width: f32,
    progress: f32,
    color: u32,
    alpha: f32,
    rotation: f32,
) {
    if progress <= 0.0 {
        return;
    }
    if progress >= 0.995 {
        draw_aa_ring(pixels, size, cx, cy, radius, width, color, alpha);
        return;
    }
    let sweep = std::f32::consts::TAU * progress;
    let half = width / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let mut angle = dy.atan2(dx) + std::f32::consts::FRAC_PI_2 - rotation;
            if angle < 0.0 {
                angle += std::f32::consts::TAU;
            }
            if angle > sweep {
                continue;
            }
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (half + 0.7 - (dist - radius).abs()).clamp(0.0, 1.0) * alpha;
            if coverage > 0.0 {
                blend_pixel(pixels, size, x, y, color, coverage);
            }
        }
    }

    let start_angle = -std::f32::consts::FRAC_PI_2 + rotation;
    let start_x = cx + radius * start_angle.cos();
    let start_y = cy + radius * start_angle.sin();
    let end_angle = -std::f32::consts::FRAC_PI_2 + sweep + rotation;
    let end_x = cx + radius * end_angle.cos();
    let end_y = cy + radius * end_angle.sin();
    draw_aa_dot(pixels, size, start_x, start_y, half, color, alpha);
    draw_aa_dot(pixels, size, end_x, end_y, half, color, alpha);
}

fn draw_aa_line(
    pixels: &mut [u8],
    size: i32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    stroke: f32,
    color: u32,
    alpha: f32,
) {
    let min_x = (x1.min(x2) - stroke).floor().max(0.0) as i32;
    let max_x = (x1.max(x2) + stroke).ceil().min((size - 1) as f32) as i32;
    let min_y = (y1.min(y2) - stroke).floor().max(0.0) as i32;
    let max_y = (y1.max(y2) + stroke).ceil().min((size - 1) as f32) as i32;
    let vx = x2 - x1;
    let vy = y2 - y1;
    let len_sq = (vx * vx + vy * vy).max(0.01);
    let radius = stroke / 2.0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let t = (((px - x1) * vx + (py - y1) * vy) / len_sq).clamp(0.0, 1.0);
            let nx = x1 + vx * t;
            let ny = y1 + vy * t;
            let dx = px - nx;
            let dy = py - ny;
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.7 - dist).clamp(0.0, 1.0) * alpha;
            if coverage > 0.0 {
                blend_pixel(pixels, size, x, y, color, coverage);
            }
        }
    }
}

fn draw_aa_dot(
    pixels: &mut [u8],
    size: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    color: u32,
    alpha: f32,
) {
    let min_x = (cx - radius - 1.0).floor().max(0.0) as i32;
    let max_x = (cx + radius + 1.0).ceil().min((size - 1) as f32) as i32;
    let min_y = (cy - radius - 1.0).floor().max(0.0) as i32;
    let max_y = (cy + radius + 1.0).ceil().min((size - 1) as f32) as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.7 - dist).clamp(0.0, 1.0) * alpha;
            if coverage > 0.0 {
                blend_pixel(pixels, size, x, y, color, coverage);
            }
        }
    }
}

fn blend_pixel(pixels: &mut [u8], size: i32, x: i32, y: i32, color: u32, alpha: f32) {
    if x < 0 || y < 0 || x >= size || y >= size {
        return;
    }
    let src_a = alpha.clamp(0.0, 1.0);
    if src_a <= 0.0 {
        return;
    }
    let index = ((y * size + x) * 4) as usize;
    let sr = (color & 0xff) as f32;
    let sg = ((color >> 8) & 0xff) as f32;
    let sb = ((color >> 16) & 0xff) as f32;
    let inv = 1.0 - src_a;
    pixels[index] = (sb * src_a + pixels[index] as f32 * inv)
        .round()
        .clamp(0.0, 255.0) as u8;
    pixels[index + 1] = (sg * src_a + pixels[index + 1] as f32 * inv)
        .round()
        .clamp(0.0, 255.0) as u8;
    pixels[index + 2] = (sr * src_a + pixels[index + 2] as f32 * inv)
        .round()
        .clamp(0.0, 255.0) as u8;
    pixels[index + 3] = ((src_a + (pixels[index + 3] as f32 / 255.0) * inv) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
}

impl IconKind {
    fn glyph(self) -> &'static str {
        match self {
            Self::Plus => "\u{E710}",
            Self::Back => "\u{E76B}",
            Self::Forward => "\u{E76C}",
            Self::Reload => "\u{E72C}",
            Self::Extensions => "\u{ECAA}",
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
    BRUSH_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        let brush = *c.brushes.entry(color).or_insert_with(|| solid_brush(color));
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
    });
}

unsafe fn fill_rect(hdc: HDC, rect: RECT, color: u32) {
    BRUSH_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        let brush = *c.brushes.entry(color).or_insert_with(|| solid_brush(color));
        let _ = FillRect(hdc, &rect, brush);
    });
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

unsafe fn draw_tab_favicon(hdc: HDC, font: &HFONT, rect: RECT, tab: &Tab, dimmed: bool) {
    if let Some(favicon) = tab.favicon_bitmap.as_ref() {
        draw_bitmap_fit(hdc, rect, favicon, dimmed);
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
    let color = if dimmed { 0x555555 } else { COLOR_ACCENT };
    draw_centered_text(hdc, font, &letter, rect, color);
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

fn decode_favicon_file(path: &str) -> Option<FaviconBitmap> {
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let path_w = to_wide(path);
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(path_w.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
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

fn favicon_candidate_urls(url: &str, known_favicon_uri: Option<&str>) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(known) = known_favicon_uri {
        let known = known.trim();
        if known.starts_with("http://") || known.starts_with("https://") {
            candidates.push(known.to_string());
        }
    }
    if let Some(origin) = url_origin(url) {
        candidates.push(format!("{origin}/favicon.ico"));
    }
    candidates.dedup();
    candidates
}

fn url_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();
    if host.is_empty() {
        None
    } else {
        Some(format!("{scheme}://{host}"))
    }
}

fn download_first_favicon(candidates: &[String], page_url: &str) -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut result = None;
    for candidate in candidates {
        if let Some(path) = download_url_to_cache(candidate) {
            if looks_like_favicon_file(&path) {
                result = Some(path);
                break;
            }
        }
    }
    if result.is_none() {
        if let Some(path) = download_url_to_cache(page_url) {
            if let Ok(html) = fs::read_to_string(&path) {
                for href in extract_favicon_hrefs(&html) {
                    if let Some(icon_url) = resolve_url(page_url, &href) {
                        if let Some(icon_path) = download_url_to_cache(&icon_url) {
                            if looks_like_favicon_file(&icon_path) {
                                result = Some(icon_path);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    unsafe {
        CoUninitialize();
    }
    result
}

fn looks_like_favicon_file(path: &str) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(&[0x00, 0x00, 0x01, 0x00])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
}

fn extract_favicon_hrefs(html: &str) -> Vec<String> {
    let mut hrefs = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(start) = lower[offset..].find("<link") {
        let tag_start = offset + start;
        let Some(tag_end_rel) = lower[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + tag_end_rel + 1;
        let tag = &html[tag_start..tag_end];
        let rel = extract_attr_value(tag, "rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if rel
            .split_whitespace()
            .any(|part| part == "icon" || part == "apple-touch-icon" || part == "mask-icon")
            || rel == "shortcut icon"
        {
            if let Some(href) = extract_attr_value(tag, "href") {
                hrefs.push(href);
            }
        }
        offset = tag_end;
    }
    hrefs
}

fn extract_attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find(attr)?;
    let after_attr = &tag[pos + attr.len()..];
    let after_attr = after_attr.trim_start();
    let value = after_attr.strip_prefix('=')?.trim_start();
    if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = value.strip_prefix('\'') {
        let end = rest.find('\'')?;
        Some(rest[..end].to_string())
    } else {
        let end = value
            .find(|ch: char| ch.is_whitespace() || ch == '>')
            .unwrap_or(value.len());
        Some(value[..end].to_string())
    }
}

fn resolve_url(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    if href.starts_with("//") {
        let scheme = base.split_once("://").map(|(scheme, _)| scheme).unwrap_or("https");
        return Some(format!("{scheme}:{href}"));
    }
    let origin = url_origin(base)?;
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    let mut prefix = base
        .split(['?', '#'])
        .next()
        .unwrap_or(base)
        .to_string();
    if !prefix.ends_with('/') {
        if let Some(pos) = prefix.rfind('/') {
            prefix.truncate(pos + 1);
        } else {
            prefix.push('/');
        }
    }
    Some(format!("{prefix}{href}"))
}

fn download_url_to_cache(url: &str) -> Option<String> {
    let url_w = to_wide(url);
    let mut path = vec![0u16; 2048];
    let ok = unsafe {
        URLDownloadToCacheFileW(
            None::<&IUnknown>,
            PCWSTR(url_w.as_ptr()),
            &mut path,
            0,
            None::<&IBindStatusCallback>,
        )
        .is_ok()
    };
    if !ok {
        return None;
    }
    let len = path.iter().position(|ch| *ch == 0).unwrap_or(path.len());
    if len == 0 {
        None
    } else {
        Some(String::from_utf16_lossy(&path[..len]))
    }
}

fn render_glyph_favicon(
    size: i32,
    codepoint: u32,
    icon_font: &HFONT,
    color: u32,
) -> Option<FaviconBitmap> {
    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return None;
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = ptr::null_mut();
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(hdc);
            return None;
        }
        let old = SelectObject(hdc, HGDIOBJ(bitmap.0));
        ptr::write_bytes(bits as *mut u8, 0, (size * size * 4) as usize);
        draw_icon_glyph(
            hdc,
            icon_font,
            &glyph(codepoint),
            RECT {
                left: 0,
                top: 0,
                right: size,
                bottom: size,
            },
            color,
        );
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
        for chunk in pixels.chunks_exact_mut(4) {
            if chunk[0] != 0 || chunk[1] != 0 || chunk[2] != 0 {
                chunk[3] = 255;
            }
        }
        SelectObject(hdc, old);
        let _ = DeleteDC(hdc);
        Some(FaviconBitmap {
            handle: bitmap,
            width: size,
            height: size,
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
    let dot_radius = (0.9 * scale).max(0.8);
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

fn download_snapshot(operation: &ICoreWebView2DownloadOperation) -> DownloadSnapshot {
    unsafe {
        let mut uri = PWSTR::null();
        let uri = if operation.Uri(&mut uri).is_ok() {
            CoTaskMemPWSTR::from(uri).to_string()
        } else {
            String::new()
        };
        let mut file_path = PWSTR::null();
        let file_path = if operation.ResultFilePath(&mut file_path).is_ok() {
            CoTaskMemPWSTR::from(file_path).to_string()
        } else {
            String::new()
        };
        let mut received_bytes = 0;
        let _ = operation.BytesReceived(&mut received_bytes);
        let mut total_bytes = 0;
        let _ = operation.TotalBytesToReceive(&mut total_bytes);
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
        let _ = operation.State(&mut state);
        DownloadSnapshot {
            uri,
            file_path,
            received_bytes,
            total_bytes,
            state,
        }
    }
}

fn download_file_name(file_path: &str, uri: &str) -> String {
    Path::new(file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .or_else(|| {
            uri.rsplit('/')
                .find(|part| !part.is_empty())
                .map(|part| part.split('?').next().unwrap_or(part).to_string())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "download".to_string())
}

fn format_bytes_split(bytes: i64) -> (String, String) {
    let bytes = bytes.max(0) as f64;
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes;
    let mut unit = 0;
    while size >= 1024.0 && unit < units.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    let val = if unit == 0 {
        format!("{}", size.round() as i64)
    } else {
        format!("{:.1}", size)
    };
    (val, units[unit].to_string())
}

fn download_state_label(download: &DownloadItem) -> &'static str {
    if download.paused {
        return "Paused";
    }
    if download.state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
        "Complete"
    } else if download.state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
        "Cancelled"
    } else {
        "Downloading"
    }
}

fn open_in_file_explorer(file_path: &str) {
    if file_path.is_empty() {
        return;
    }
    let path = Path::new(file_path);

    let full_path = if path.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            cwd.join(path)
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };

    if full_path.exists() {
        let _ = Command::new("explorer.exe")
            .arg(format!("/select,{}", full_path.display()))
            .spawn();
    } else if let Some(parent) = full_path.parent() {
        let _ = Command::new("explorer.exe").arg(parent.as_os_str()).spawn();
    }
}

fn decode_upload_content(content: &str) -> std::result::Result<Vec<u8>, String> {
    let payload = content
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or(content)
        .trim();
    general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| error.to_string())
}

fn looks_like_sqlite(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    bytes.starts_with(b"SQLite format 3")
        || lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
        || lower.ends_with(".db")
        || lower == "history"
        || lower == "cookies"
        || lower.ends_with("\\history")
        || lower.ends_with("\\cookies")
        || lower.contains("places.sqlite")
}

fn looks_like_html(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".html")
        || lower.ends_with(".htm")
        || std::str::from_utf8(bytes)
            .map(|text| text.to_ascii_lowercase().contains("netscape-bookmark-file"))
            .unwrap_or(false)
}

fn sqlite_temp_path(name: &str, path_hint: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let raw_name = Path::new(path_hint)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(name);
    let safe_name: String = raw_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    path.push(format!(
        "aster-import-{}-{}-{}.sqlite",
        std::process::id(),
        current_timestamp(),
        safe_name
    ));
    path
}

fn sqlite_table_names(connection: &Connection) -> Vec<String> {
    let Ok(mut statement) =
        connection.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
    else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(|row| row.ok()).collect()
}

fn chrome_time_to_unix_secs(value: i64) -> u64 {
    const CHROME_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_i64 * 1_000_000;
    if value <= CHROME_EPOCH_OFFSET_MICROS {
        0
    } else {
        ((value - CHROME_EPOCH_OFFSET_MICROS) / 1_000_000) as u64
    }
}

fn same_site_from_chromium(value: i32) -> String {
    match value {
        1 => "lax",
        2 => "strict",
        _ => "none",
    }
    .to_string()
}

fn same_site_from_firefox(value: i32) -> String {
    match value {
        1 => "lax",
        2 => "strict",
        _ => "none",
    }
    .to_string()
}

fn same_site_from_webview(value: COREWEBVIEW2_COOKIE_SAME_SITE_KIND) -> String {
    if value == COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX {
        "lax".to_string()
    } else if value == COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT {
        "strict".to_string()
    } else {
        "none".to_string()
    }
}

fn same_site_to_webview(value: &str) -> COREWEBVIEW2_COOKIE_SAME_SITE_KIND {
    match value.to_ascii_lowercase().as_str() {
        "lax" => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
        "strict" => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
        _ => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE,
    }
}

fn dpapi_decrypt(bytes: &[u8]) -> std::result::Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("empty encrypted payload".to_string());
    }
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output)
            .map_err(|error| error.to_string())?;
        let result = if output.pbData.is_null() || output.cbData == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        let _ = LocalFree(Some(HLOCAL(output.pbData as _)));
        Ok(result)
    }
}

fn extract_chromium_profile_key(bytes: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    let encrypted_key = value
        .get("os_crypt")?
        .get("encrypted_key")?
        .as_str()
        .filter(|key| !key.is_empty())?;
    let mut decoded = general_purpose::STANDARD.decode(encrypted_key).ok()?;
    if decoded.starts_with(b"DPAPI") {
        decoded.drain(0..5);
    }
    dpapi_decrypt(&decoded).ok()
}

fn decrypt_chromium_cookie(bytes: &[u8], key: &[u8]) -> std::result::Result<String, String> {
    if bytes.is_empty() {
        return Err("empty encrypted cookie".to_string());
    }
    if (bytes.starts_with(b"v10") || bytes.starts_with(b"v11")) && bytes.len() > 15 {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|error| error.to_string())?;
        let nonce = Nonce::from_slice(&bytes[3..15]);
        let decrypted = cipher
            .decrypt(nonce, &bytes[15..])
            .map_err(|error| error.to_string())?;
        String::from_utf8(decrypted).map_err(|error| error.to_string())
    } else {
        let decrypted = dpapi_decrypt(bytes)?;
        String::from_utf8(decrypted).map_err(|error| error.to_string())
    }
}

fn portable_cookie_from_webview(cookie: &ICoreWebView2Cookie) -> Option<PortableCookie> {
    unsafe {
        let mut raw = PWSTR::null();
        cookie.Name(&mut raw).ok()?;
        let name = take_pwstr(raw);
        cookie.Value(&mut raw).ok()?;
        let value = take_pwstr(raw);
        cookie.Domain(&mut raw).ok()?;
        let domain = take_pwstr(raw);
        cookie.Path(&mut raw).ok()?;
        let path = take_pwstr(raw);

        let mut expires = 0.0f64;
        let _ = cookie.Expires(&mut expires);
        let mut session = BOOL(0);
        let _ = cookie.IsSession(&mut session);
        let mut secure = BOOL(0);
        let _ = cookie.IsSecure(&mut secure);
        let mut http_only = BOOL(0);
        let _ = cookie.IsHttpOnly(&mut http_only);
        let mut same_site = COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE;
        let _ = cookie.SameSite(&mut same_site);

        Some(PortableCookie {
            name,
            value,
            domain,
            path,
            expires: if session.as_bool() {
                None
            } else {
                Some(expires)
            },
            secure: secure.as_bool(),
            http_only: http_only.as_bool(),
            same_site: same_site_from_webview(same_site),
        })
    }
}

fn is_account_cookie(cookie: &PortableCookie) -> bool {
    let domain = cookie.domain.to_ascii_lowercase();
    if !(domain.contains("google.") || domain.contains("accounts.google.")) {
        return false;
    }
    matches!(
        cookie.name.as_str(),
        "SID"
            | "HSID"
            | "SSID"
            | "APISID"
            | "SAPISID"
            | "__Secure-1PSID"
            | "__Secure-3PSID"
            | "__Secure-1PSIDTS"
            | "__Secure-3PSIDTS"
            | "LSID"
            | "ACCOUNT_CHOOSER"
    )
}

fn collect_bookmarks_from_json(value: &Value, folder: &str, out: &mut Vec<PortableBookmark>) {
    match value {
        Value::Object(map) => {
            let name = map
                .get("name")
                .or_else(|| map.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let url = map
                .get("url")
                .or_else(|| map.get("uri"))
                .and_then(Value::as_str);
            if let Some(url) = url {
                if !url.trim().is_empty() {
                    out.push(PortableBookmark {
                        title: if name.trim().is_empty() {
                            label_for_url(url)
                        } else {
                            name.to_string()
                        },
                        url: url.to_string(),
                        folder: folder.to_string(),
                        tags: Vec::new(),
                        created_at: current_timestamp(),
                    });
                }
            }
            let next_folder = if url.is_none() && !name.trim().is_empty() {
                name
            } else {
                folder
            };
            for child in map.values() {
                collect_bookmarks_from_json(child, next_folder, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_bookmarks_from_json(item, folder, out);
            }
        }
        _ => {}
    }
}

fn collect_history_from_json(value: &Value, out: &mut Vec<PortableHistoryEntry>) {
    match value {
        Value::Object(map) => {
            let has_history_shape = map.contains_key("visit_count")
                || map.contains_key("visitCount")
                || map.contains_key("last_visit_time")
                || map.contains_key("lastVisitTime")
                || map.contains_key("last_visit_date");
            if has_history_shape {
                if let Some(url) = map.get("url").and_then(Value::as_str) {
                    let title = map
                        .get("title")
                        .or_else(|| map.get("name"))
                        .and_then(Value::as_str)
                        .map(|title| title.to_string())
                        .unwrap_or_else(|| label_for_url(url));
                    out.push(PortableHistoryEntry {
                        title,
                        url: url.to_string(),
                        visit_count: map
                            .get("visit_count")
                            .or_else(|| map.get("visitCount"))
                            .and_then(Value::as_u64)
                            .unwrap_or(1)
                            .max(1) as u32,
                        last_visit_time: map
                            .get("last_visit_time")
                            .or_else(|| map.get("lastVisitTime"))
                            .or_else(|| map.get("last_visit_date"))
                            .and_then(Value::as_u64)
                            .unwrap_or_else(current_timestamp),
                    });
                }
            }
            for child in map.values() {
                collect_history_from_json(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_history_from_json(item, out);
            }
        }
        _ => {}
    }
}

fn collect_cookies_from_json(value: &Value, out: &mut Vec<PortableCookie>) {
    match value {
        Value::Object(map) => {
            let domain = map
                .get("domain")
                .or_else(|| map.get("host"))
                .or_else(|| map.get("host_key"))
                .and_then(Value::as_str);
            let name = map.get("name").and_then(Value::as_str);
            let value_text = map.get("value").and_then(Value::as_str);
            if let (Some(domain), Some(name), Some(value_text)) = (domain, name, value_text) {
                out.push(PortableCookie {
                    name: name.to_string(),
                    value: value_text.to_string(),
                    domain: domain.to_string(),
                    path: map
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("/")
                        .to_string(),
                    expires: map
                        .get("expires")
                        .or_else(|| map.get("expirationDate"))
                        .or_else(|| map.get("expiry"))
                        .and_then(Value::as_f64),
                    secure: map
                        .get("secure")
                        .or_else(|| map.get("isSecure"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    http_only: map
                        .get("http_only")
                        .or_else(|| map.get("httpOnly"))
                        .or_else(|| map.get("isHttpOnly"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    same_site: map
                        .get("same_site")
                        .or_else(|| map.get("sameSite"))
                        .and_then(Value::as_str)
                        .unwrap_or("none")
                        .to_string(),
                });
            }
            for child in map.values() {
                collect_cookies_from_json(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_cookies_from_json(item, out);
            }
        }
        _ => {}
    }
}

fn parse_bookmark_html(raw: &str) -> Vec<PortableBookmark> {
    let mut bookmarks = Vec::new();
    let lower = raw.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(rel_start) = lower[cursor..].find("<a ") {
        let start = cursor + rel_start;
        let Some(rel_end) = lower[start..].find('>') else {
            break;
        };
        let tag_end = start + rel_end;
        let tag = &raw[start..=tag_end];
        let Some(href) = html_attr(tag, "href") else {
            cursor = tag_end + 1;
            continue;
        };
        let Some(rel_close) = lower[tag_end..].find("</a>") else {
            break;
        };
        let close = tag_end + rel_close;
        let title = html_unescape(raw[tag_end + 1..close].trim());
        bookmarks.push(PortableBookmark {
            title: if title.is_empty() {
                label_for_url(&href)
            } else {
                title
            },
            url: html_unescape(&href),
            folder: String::new(),
            tags: Vec::new(),
            created_at: current_timestamp(),
        });
        cursor = close + 4;
    }
    bookmarks
}

fn html_attr(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let pos = lower.find(&needle)? + needle.len();
    let bytes = tag.as_bytes();
    let quote = bytes.get(pos).copied().unwrap_or(b' ');
    if quote == b'"' || quote == b'\'' {
        let start = pos + 1;
        let end = tag[start..].find(quote as char)? + start;
        Some(tag[start..end].to_string())
    } else {
        let end = tag[pos..]
            .find(|ch: char| ch.is_whitespace() || ch == '>')
            .map(|offset| pos + offset)
            .unwrap_or(tag.len());
        Some(tag[pos..end].to_string())
    }
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn html_escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_escape_attr(value: &str) -> String {
    html_escape_text(value).replace('"', "&quot;")
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

fn settings_page_html(
    dominant_color: u32,
    secondary_color: u32,
    accent_color: u32,
    site_mode: &str,
    startup_mode: &str,
    is_default: bool,
    import_notice: &str,
) -> String {
    let dominant = colorref_to_css(dominant_color);
    let secondary = colorref_to_css(secondary_color);
    let accent = colorref_to_css(accent_color);
    let import_notice_html = if import_notice.trim().is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="notice" id="importNotice"><span>{}</span><button class="notice-close" id="clearImportNotice">Close</button></div>"#,
            html_escape_text(import_notice)
        )
    };
    let default_browser_button = if is_default {
        r#"<button class="action-btn" id="makeDefaultBtn" disabled style="opacity: 0.6; cursor: not-allowed;">Aster is default</button>"#
    } else {
        r#"<button class="action-btn" id="makeDefaultBtn">Set as default</button>"#
    };
    format!(
        r##"<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Aster Settings</title>
<style>
:root {{ --accent: {accent}; --bg: {dominant}; --secondary: {secondary}; --panel: {secondary}; --line: #2a2a2a; --text: #f5f5f5; --muted: #a1a1a1; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 "Segoe UI Variable Text", "Segoe UI", sans-serif; }}
.shell {{ display: grid; grid-template-columns: 224px minmax(0, 1fr); min-height: 100vh; }}
nav {{ border-right: 1px solid var(--line); padding: 28px 14px; background: #080808; }}
h1 {{ font-size: 20px; margin: 0 0 22px; font-weight: 650; }}
button, input, select {{ font: inherit; }}
.tab {{ width: 100%; border: 0; color: var(--muted); background: transparent; text-align: left; padding: 10px 12px; border-radius: 8px; cursor: pointer; }}
.tab.active, .tab:hover {{ color: var(--text); background: #181818; }}
main {{ padding: 34px clamp(24px, 5vw, 72px); max-width: 980px; }}
section {{ display: none; }}
section.active {{ display: block; }}
h2 {{ font-size: 24px; margin: 0 0 6px; }}
.lead {{ color: var(--muted); margin: 0 0 26px; }}
.group {{ border: 1px solid var(--line); border-radius: 8px; overflow: hidden; margin: 16px 0; background: var(--panel); }}
.row {{ display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 18px; align-items: center; padding: 16px 18px; border-top: 1px solid var(--line); }}
.row:first-child {{ border-top: 0; }}
.title {{ font-weight: 600; }}
.hint {{ color: var(--muted); font-size: 12px; margin-top: 3px; }}
input[type=color] {{ width: 44px; height: 32px; border: 1px solid var(--line); background: transparent; border-radius: 6px; padding: 2px; }}
select, .capture {{ min-width: 170px; color: var(--text); background: #080808; border: 1px solid var(--line); border-radius: 7px; padding: 8px 10px; }}
.capture.recording {{ border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent), transparent 70%); }}
.reset-btn {{ width: 32px; height: 32px; border: 1px solid var(--line); border-radius: 6px; background: #080808; color: var(--muted); cursor: pointer; display: inline-flex; align-items: center; justify-content: center; font-size: 16px; }}
.reset-btn:hover {{ color: var(--text); background: var(--panel); }}
.action-btn {{ min-width: 130px; color: var(--text); background: #080808; border: 1px solid var(--line); border-radius: 7px; padding: 8px 10px; cursor: pointer; }}
.action-btn:hover {{ background: var(--panel); }}
.pill {{ display: inline-flex; align-items: center; gap: 8px; color: var(--text); background: #080808; border: 1px solid var(--line); border-radius: 999px; padding: 7px 11px; }}
.dot {{ width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }}
.stack {{ display: grid; gap: 12px; }}
.actions {{ display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }}
.dropzone {{ border: 1px dashed color-mix(in srgb, var(--accent), var(--line) 55%); border-radius: 8px; padding: 18px; background: color-mix(in srgb, var(--panel), #000 14%); min-width: min(420px, 100%); transition: border-color .16s ease, background .16s ease; }}
.dropzone.dragging {{ border-color: var(--accent); background: color-mix(in srgb, var(--accent), transparent 90%); }}
.drop-title {{ font-weight: 600; }}
.drop-hint {{ color: var(--muted); font-size: 12px; margin-top: 4px; max-width: 540px; }}
.notice {{ display: flex; align-items: center; justify-content: space-between; gap: 12px; border: 1px solid color-mix(in srgb, var(--accent), var(--line) 58%); background: color-mix(in srgb, var(--accent), transparent 90%); border-radius: 8px; padding: 11px 13px; margin: 0 0 14px; }}
.notice-close {{ color: var(--muted); border: 0; background: transparent; cursor: pointer; }}
.notice-close:hover {{ color: var(--text); }}
.file-input {{ display: none; }}
.status {{ color: var(--muted); font-size: 12px; min-height: 18px; }}
@media (max-width: 760px) {{ .row {{ grid-template-columns: minmax(0, 1fr); }} .actions {{ justify-content: stretch; }} .actions .action-btn {{ flex: 1; }} }}
</style>
</head>
<body>
<div class="shell">
<nav>
<h1>Aster Settings</h1>
<button class="tab active" data-tab="general">General</button>
<button class="tab" data-tab="appearance">Appearance</button>
<button class="tab" data-tab="keybinds">Keybinds</button>
<button class="tab" data-tab="privacy">Privacy</button>
</nav>
<main>
<section id="general" class="active">
<h2>General</h2><p class="lead">Startup behavior and preferences.</p>
<div class="group">
<div class="row"><div><div class="title">Startup page</div><div class="hint">Choose what opens when Aster starts.</div></div><select id="startupMode"><option value="home">Home page</option><option value="last">Last session</option></select></div>
<div class="row"><div><div class="title">Default browser</div><div class="hint">Make Aster your default web browser.</div></div>{default_browser_button}</div>
</div>
</section>
<section id="appearance">
<h2>Appearance</h2><p class="lead">Tune Aster's browser chrome and page preference.</p>
<div class="group">
<div class="row"><div><div class="title">Primary</div><div class="hint">The main browser background color.</div></div><div style="display:flex;gap:6px;align-items:center"><input id="dominant" type="color" value="{dominant}"><button class="reset-btn" data-target="dominant">↺</button></div></div>
<div class="row"><div><div class="title">Secondary</div><div class="hint">The chrome panel and sidebar color.</div></div><div style="display:flex;gap:6px;align-items:center"><input id="secondary" type="color" value="{secondary}"><button class="reset-btn" data-target="secondary">↺</button></div></div>
<div class="row"><div><div class="title">Accent</div><div class="hint">Used for highlights, active states, and find-in-page marks.</div></div><div style="display:flex;gap:6px;align-items:center"><input id="accent" type="color" value="{accent}"><button class="reset-btn" data-target="accent">↺</button></div></div>
<div class="row"><div><div class="title">Site theme</div><div class="hint">Preferred color scheme for webpages.</div></div><select id="siteMode"><option value="auto">Auto</option><option value="dark">Dark</option><option value="light">Light</option></select></div>
</div>
</section>
<section id="keybinds">
<h2>Keybinds</h2><p class="lead">Click a shortcut, then press the new combination.</p>
<div class="group" id="keybindRows"></div>
</section>
<section id="privacy">
<h2>Privacy</h2><p class="lead">Site data and browsing controls.</p>
{import_notice_html}
<div class="group">
<div class="row"><div><div class="title">Browser data</div><div class="hint">View your saved bookmarks, tabs, and settings.</div></div><button class="action-btn" id="openStateFile">Open aster-state</button></div>
<div class="row"><div><div class="title">Import from another browser</div><div class="hint">Drop bookmark JSON/HTML, history SQLite, cookies SQLite, Aster exports, or a matching Chromium Local State file for account sessions.</div></div><div class="stack"><div class="dropzone" id="importDrop"><div class="drop-title">Drop browser data files here</div><div class="drop-hint">Works with Chrome, Edge, Firefox, and Aster export files. Drop multiple files together when sessions need a profile key.</div></div><div class="actions"><button class="action-btn" id="chooseImport">Choose files</button></div><input class="file-input" id="importInput" type="file" multiple><div class="status" id="importStatus"></div></div></div>
<div class="row"><div><div class="title">Export data</div><div class="hint">Aster data restores bookmarks, history, and sessions in Aster. Bookmarks HTML can be imported by other browsers.</div></div><div class="actions"><button class="action-btn" id="exportAsterData">Export Aster data</button><button class="action-btn" id="exportBookmarkHtml">Export bookmarks HTML</button></div></div>
</div>
</section>
</main>
</div>
<script>
const post = (m) => window.chrome?.webview?.postMessage(m);
document.querySelectorAll(".tab").forEach((tab) => tab.onclick = () => {{
  document.querySelectorAll(".tab, section").forEach((el) => el.classList.remove("active"));
  tab.classList.add("active");
  document.getElementById(tab.dataset.tab).classList.add("active");
}});
const siteMode = document.getElementById("siteMode");
siteMode.value = "{site_mode_lc}";
siteMode.onchange = () => post("settings:site-mode:" + siteMode.value);
const startupMode = document.getElementById("startupMode");
startupMode.value = "{startup_mode}";
startupMode.onchange = () => post("settings:startup:" + startupMode.value);
document.getElementById("dominant").oninput = (e) => {{ document.documentElement.style.setProperty("--bg", e.target.value); post("settings:dominant:" + e.target.value); }};
document.getElementById("secondary").oninput = (e) => {{ document.documentElement.style.setProperty("--secondary", e.target.value); document.documentElement.style.setProperty("--panel", e.target.value); post("settings:secondary:" + e.target.value); }};
document.getElementById("accent").oninput = (e) => {{ document.documentElement.style.setProperty("--accent", e.target.value); post("settings:accent:" + e.target.value); }};
document.querySelectorAll(".reset-btn").forEach((btn) => {{
  btn.onclick = () => {{
    const target = btn.dataset.target;
    if (target === "dominant") {{
      document.getElementById("dominant").value = "#000000";
      document.documentElement.style.setProperty("--bg", "#000000");
      post("settings:dominant:#000000");
    }} else if (target === "secondary") {{
      document.getElementById("secondary").value = "#090909";
      document.documentElement.style.setProperty("--secondary", "#090909");
      document.documentElement.style.setProperty("--panel", "#090909");
      post("settings:secondary:#090909");
    }} else if (target === "accent") {{
      document.getElementById("accent").value = "#636ff1";
      document.documentElement.style.setProperty("--accent", "#636ff1");
      post("settings:accent:#636ff1");
    }}
  }};
}});
document.getElementById("openStateFile").onclick = () => post("settings:open-state-file");
const makeDefaultBtn = document.getElementById("makeDefaultBtn");
if (makeDefaultBtn) makeDefaultBtn.onclick = () => post("settings:make-default");
const importStatus = document.getElementById("importStatus");
const clearImportNotice = document.getElementById("clearImportNotice");
if (clearImportNotice) clearImportNotice.onclick = () => post("settings:clear-import-notice");
function dateStamp() {{
  return new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
}}
function downloadText(filename, mime, text) {{
  const blob = new Blob([text], {{ type: mime }});
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 800);
}}
window.asterReceiveExport = (kind, mime, text) => {{
  if (kind === "aster") {{
    downloadText(`aster-data-${{dateStamp()}}.json`, mime, text);
    importStatus.textContent = "Aster data export ready.";
  }} else if (kind === "bookmarks") {{
    downloadText(`aster-bookmarks-${{dateStamp()}}.html`, mime, text);
    importStatus.textContent = "Bookmarks export ready.";
  }}
}};
document.getElementById("exportAsterData").onclick = () => {{
  importStatus.textContent = "Preparing Aster data export...";
  post("settings:export:aster");
}};
document.getElementById("exportBookmarkHtml").onclick = () => {{
  importStatus.textContent = "Preparing bookmarks export...";
  post("settings:export:bookmarks");
}};
const importDrop = document.getElementById("importDrop");
const importInput = document.getElementById("importInput");
document.getElementById("chooseImport").onclick = () => importInput.click();
importInput.onchange = () => importFiles(importInput.files);
["dragenter", "dragover"].forEach((name) => importDrop.addEventListener(name, (event) => {{
  event.preventDefault();
  importDrop.classList.add("dragging");
}}));
["dragleave", "drop"].forEach((name) => importDrop.addEventListener(name, (event) => {{
  event.preventDefault();
  if (name === "drop") importFiles(event.dataTransfer.files);
  importDrop.classList.remove("dragging");
}}));
function readFileData(file) {{
  return new Promise((resolve, reject) => {{
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || "").split(",").pop() || "");
    reader.onerror = () => reject(reader.error || new Error("Could not read file"));
    reader.readAsDataURL(file);
  }});
}}
async function importFiles(files) {{
  const list = Array.from(files || []);
  if (!list.length) return;
  importStatus.textContent = `Preparing ${{list.length}} file${{list.length === 1 ? "" : "s"}}...`;
  try {{
    const payload = [];
    for (const file of list) {{
      payload.push({{
        name: file.name,
        path: file.webkitRelativePath || file.name,
        content: await readFileData(file)
      }});
    }}
    importStatus.textContent = "Importing...";
    post("settings:import-files:" + JSON.stringify(payload));
  }} catch (error) {{
    importStatus.textContent = error && error.message ? error.message : "Import failed";
  }}
}}
const defaults = [
  ["Navigate", "Ctrl+L"], ["Bookmark site", "Ctrl+D"], ["Find in page", "Ctrl+F"], ["New tab", "Ctrl+T"],
  ["Close tab", "Ctrl+W"], ["Reload", "Ctrl+R"], ["Reset zoom", "Ctrl+0"], ["Zoom in", "Ctrl++"],
  ["Zoom out", "Ctrl+-"], ["Reopen closed tab", "Ctrl+Shift+Z"], ["Toggle sidebar", "Ctrl+S"],
  ["Go back", "Alt+A"], ["Go forward", "Alt+D"], ["Switch tab above", "Alt+W"],
  ["Switch tab below", "Alt+S"], ["Toggle fullscreen", "F11"]
];
const rows = document.getElementById("keybindRows");
defaults.forEach(([name, combo]) => {{
  let saved = combo;
  try {{ saved = localStorage.getItem("aster.keybind." + name) || combo; }} catch {{}}
  const row = document.createElement("div");
  row.className = "row";
  row.innerHTML = `<div><div class="title">${{name}}</div><div class="hint">Current shortcut</div></div><button class="capture">${{saved}}</button>`;
  const button = row.querySelector("button");
  button.onclick = () => {{ button.textContent = "Press keys"; button.classList.add("recording"); button.focus(); }};
  button.onblur = () => {{ button.classList.remove("recording"); button.textContent = saved; }};
  button.onkeydown = (event) => {{
    event.preventDefault();
    const parts = [];
    if (event.ctrlKey) parts.push("Ctrl");
    if (event.shiftKey) parts.push("Shift");
    if (event.altKey) parts.push("Alt");
    if (!["Control","Shift","Alt"].includes(event.key)) parts.push(event.key.length === 1 ? event.key.toUpperCase() : event.key);
    const next = parts.join("+");
    if (next) {{ try {{ localStorage.setItem("aster.keybind." + name, next); }} catch {{}} saved = next; button.textContent = next; post("settings:keybind:" + name + ":" + next); }}
    button.classList.remove("recording");
  }};
  rows.appendChild(row);
}});
</script>
</body>
</html>"##,
        dominant = dominant,
        accent = accent,
        site_mode_lc = site_mode.to_ascii_lowercase(),
        startup_mode = startup_mode,
        default_browser_button = default_browser_button
    )
}

fn history_entries_json(entries: &[VisitedSite]) -> String {
    let items = entries
        .iter()
        .filter(|entry| !entry.url.trim().is_empty())
        .map(|entry| {
            let title = if entry.title.trim().is_empty() {
                label_for_url(&entry.url)
            } else {
                entry.title.clone()
            };
            format!(
                "{{\"title\":{},\"url\":{},\"visitCount\":{},\"lastVisitTime\":{}}}",
                js_string_literal(&title),
                js_string_literal(&entry.url),
                entry.visit_count,
                entry.last_visit_time
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", items)
}

fn extensions_entries_json(entries: &[BrowserExtensionInfo]) -> String {
    let items = entries
        .iter()
        .map(|entry| {
            format!(
                "{{\"id\":{},\"name\":{},\"enabled\":{}}}",
                js_string_literal(&entry.id),
                js_string_literal(&entry.name),
                if entry.enabled { "true" } else { "false" }
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", items)
}

fn extensions_page_html(
    dominant_color: u32,
    secondary_color: u32,
    accent_color: u32,
    extensions: &[BrowserExtensionInfo],
    notice: &str,
) -> String {
    let dominant = colorref_to_css(dominant_color);
    let secondary = colorref_to_css(secondary_color);
    let accent = colorref_to_css(accent_color);
    let entries_json = extensions_entries_json(extensions);
    let notice_html = if notice.trim().is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="notice"><span>{}</span></div>"#,
            html_escape_text(notice)
        )
    };
    let template = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Aster Extensions</title>
<style>
:root { --accent: __ACCENT__; --bg: __DOMINANT__; --secondary: __SECONDARY__; --panel: __SECONDARY__; --line: #2a2a2a; --text: #f5f5f5; --muted: #a1a1a1; --soft: #151515; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 "Segoe UI Variable Text", "Segoe UI", sans-serif; }
button, input { font: inherit; }
.page { min-height: 100vh; padding: 34px clamp(22px, 5vw, 72px); }
.wrap { max-width: 980px; margin: 0 auto; }
.header { display: flex; align-items: end; justify-content: space-between; gap: 16px; margin: 0 0 22px; }
h1 { font-size: 28px; line-height: 1.1; margin: 0 0 7px; font-weight: 650; }
.lead { color: var(--muted); margin: 0; }
.actions { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
.group { border: 1px solid var(--line); border-radius: 8px; overflow: hidden; background: var(--panel); margin: 16px 0; }
.row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 16px; align-items: center; padding: 15px 17px; border-top: 1px solid var(--line); }
.row:first-child { border-top: 0; }
.title { font-weight: 600; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.hint { color: var(--muted); font-size: 12px; margin-top: 3px; overflow-wrap: anywhere; }
.load { display: grid; grid-template-columns: minmax(180px, 1fr) auto; gap: 8px; width: min(560px, 100%); }
input { color: var(--text); background: #080808; border: 1px solid var(--line); border-radius: 7px; padding: 8px 10px; min-width: 0; }
.action-btn { min-width: 110px; color: var(--text); background: #080808; border: 1px solid var(--line); border-radius: 7px; padding: 8px 10px; cursor: pointer; }
.action-btn:hover { background: color-mix(in srgb, var(--accent), transparent 88%); border-color: color-mix(in srgb, var(--accent), var(--line) 55%); }
.toggle { display: inline-flex; align-items: center; gap: 8px; color: var(--muted); }
.toggle input { width: 18px; height: 18px; accent-color: var(--accent); }
.notice { border: 1px solid color-mix(in srgb, var(--accent), var(--line) 58%); background: color-mix(in srgb, var(--accent), transparent 90%); border-radius: 8px; padding: 11px 13px; margin: 0 0 14px; }
.empty { color: var(--muted); padding: 18px; }
@media (max-width: 760px) { .header, .row { grid-template-columns: minmax(0, 1fr); flex-direction: column; align-items: stretch; } .load { grid-template-columns: minmax(0, 1fr); } .actions { justify-content: stretch; } .action-btn { flex: 1; } }
</style>
</head>
<body>
<div class="page"><div class="wrap">
<div class="header"><div><h1>Extensions</h1><p class="lead">Load unpacked Chrome extensions and manage what is active in Aster.</p></div><div class="actions"><button class="action-btn" id="openStore">Chrome Web Store</button><button class="action-btn" id="refresh">Refresh</button></div></div>
__NOTICE__
<div class="group">
<div class="row"><div><div class="title">Load unpacked extension</div><div class="hint">Enter the folder that contains manifest.json. Extensions are stored by the WebView2 profile.</div></div><div class="load"><input id="extensionPath" placeholder="C:\path\to\extension"><button class="action-btn" id="loadUnpacked">Load</button></div></div>
</div>
<div class="group" id="extensionRows"></div>
</div></div>
<script>
const post = (m) => window.chrome?.webview?.postMessage(m);
const extensions = __EXTENSIONS__;
const rows = document.getElementById("extensionRows");
function render() {
  rows.innerHTML = "";
  if (!extensions.length) {
    rows.innerHTML = `<div class="empty">No extensions installed yet.</div>`;
    return;
  }
  extensions.forEach((extension) => {
    const row = document.createElement("div");
    row.className = "row";
    row.innerHTML = `<div><div class="title"></div><div class="hint"></div></div><div class="actions"><label class="toggle"><input type="checkbox"${extension.enabled ? " checked" : ""}>Enabled</label><button class="action-btn">Remove</button></div>`;
    row.querySelector(".title").textContent = extension.name;
    row.querySelector(".hint").textContent = extension.id;
    row.querySelector("input").onchange = (event) => post("extensions:enable:" + encodeURIComponent(extension.id) + ":" + (event.target.checked ? "1" : "0"));
    row.querySelector("button").onclick = () => post("extensions:remove:" + encodeURIComponent(extension.id));
    rows.appendChild(row);
  });
}
document.getElementById("openStore").onclick = () => post("extensions:open-store");
document.getElementById("refresh").onclick = () => post("extensions:refresh");
document.getElementById("loadUnpacked").onclick = () => {
  const path = document.getElementById("extensionPath").value.trim();
  if (path) post("extensions:add:" + encodeURIComponent(path));
};
render();
</script>
</body>
</html>"#;
    template
        .replace("__ACCENT__", &accent)
        .replace("__DOMINANT__", &dominant)
        .replace("__SECONDARY__", &secondary)
        .replace("__EXTENSIONS__", &entries_json)
        .replace("__NOTICE__", &notice_html)
}

fn history_page_html(
    dominant_color: u32,
    secondary_color: u32,
    accent_color: u32,
    sort_mode: HistorySortMode,
    visit_sort_desc: bool,
    entries: &[VisitedSite],
) -> String {
    let dominant = colorref_to_css(dominant_color);
    let secondary = colorref_to_css(secondary_color);
    let accent = colorref_to_css(accent_color);
    let entries_json = history_entries_json(entries);
    let visit_direction = if visit_sort_desc { "desc" } else { "asc" };
    let template = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Aster History</title>
<style>
:root { --accent: __ACCENT__; --bg: __DOMINANT__; --secondary: __SECONDARY__; --panel: __SECONDARY__; --line: #2a2a2a; --text: #f5f5f5; --muted: #a1a1a1; --soft: #151515; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.45 "Segoe UI Variable Text", "Segoe UI", sans-serif; }
button, select { font: inherit; }
.page { min-height: 100vh; padding: 34px clamp(22px, 5vw, 72px); }
.header { display: flex; align-items: end; justify-content: space-between; gap: 20px; max-width: 1040px; margin: 0 auto 26px; }
h1 { font-size: 28px; line-height: 1.1; margin: 0 0 7px; font-weight: 650; }
.lead { color: var(--muted); margin: 0; }
.controls { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; justify-content: flex-end; }
select { color: var(--text); background: #080808; border: 1px solid var(--line); border-radius: 7px; padding: 8px 10px; min-width: 148px; }
.wrap { max-width: 1040px; margin: 0 auto; }
.empty { border: 1px solid var(--line); background: var(--panel); border-radius: 8px; padding: 28px; color: var(--muted); }
.year { margin: 28px 0 0; }
.year-title { color: var(--text); font-size: 20px; font-weight: 650; margin: 0 0 14px; }
.month { margin: 0 0 20px; }
.month-title { color: var(--muted); font-size: 12px; letter-spacing: .08em; text-transform: uppercase; margin: 18px 0 10px; }
.day { border: 1px solid var(--line); background: var(--panel); border-radius: 8px; overflow: hidden; margin: 10px 0 14px; }
.day-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 12px 16px; border-bottom: 1px solid var(--line); background: color-mix(in srgb, var(--panel), #000 18%); }
.day-name { font-weight: 600; }
.day-count { color: var(--muted); font-size: 12px; }
.item { width: 100%; display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 16px; text-align: left; border: 0; border-top: 1px solid var(--line); background: transparent; color: var(--text); padding: 13px 16px; cursor: pointer; }
.item:first-of-type { border-top: 0; }
.item:hover, .item:focus-visible { background: color-mix(in srgb, var(--accent), transparent 88%); outline: none; }
.title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 560; }
.url { color: var(--muted); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; margin-top: 2px; font-size: 12px; }
.meta { color: var(--muted); font-size: 12px; white-space: nowrap; }
.dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); margin-right: 8px; vertical-align: 1px; }
@media (max-width: 720px) {
  .header { align-items: stretch; flex-direction: column; }
  .controls { justify-content: stretch; }
  select { flex: 1 1 180px; min-width: 0; }
  .item { grid-template-columns: minmax(0, 1fr); gap: 6px; }
}
</style>
</head>
<body>
<div class="page">
  <header class="header">
    <div>
      <h1>Aster History</h1>
      <p class="lead">Visited pages grouped by date.</p>
    </div>
    <div class="controls">
      <select id="sortMode" aria-label="Sort history">
        <option value="latest">Latest</option>
        <option value="oldest">Oldest</option>
        <option value="most_visited">Most visited</option>
      </select>
      <select id="visitDirection" aria-label="Most visited direction">
        <option value="desc">Most visited first</option>
        <option value="asc">Least visited first</option>
      </select>
    </div>
  </header>
  <main class="wrap" id="history"></main>
</div>
<script>
const post = (m) => window.chrome?.webview?.postMessage(m);
const entries = __ENTRIES__;
const sortMode = document.getElementById("sortMode");
const visitDirection = document.getElementById("visitDirection");
sortMode.value = "__SORT_MODE__";
visitDirection.value = "__VISIT_DIRECTION__";
function cleanUrl(url) {
  return String(url || "").replace(/^https?:\/\//, "").replace(/^www\./, "").replace(/\/$/, "");
}
function itemTitle(item) {
  return item.title && item.title !== "New Tab" ? item.title : cleanUrl(item.url);
}
function dayLabel(date) {
  const today = new Date();
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const then = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const diff = Math.round((start - then) / 86400000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  return date.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" });
}
function monthLabel(year, month) {
  return new Date(year, month, 1).toLocaleDateString(undefined, { month: "long", year: "numeric" });
}
function visitText(count) {
  return count === 1 ? "1 visit" : `${count} visits`;
}
function sortItems(items) {
  const mode = sortMode.value;
  const byVisit = visitDirection.value === "asc" ? 1 : -1;
  return items.slice().sort((a, b) => {
    if (mode === "most_visited") {
      const visits = (a.visitCount - b.visitCount) * byVisit;
      if (visits !== 0) return visits;
      return b.lastVisitTime - a.lastVisitTime;
    }
    return mode === "oldest"
      ? a.lastVisitTime - b.lastVisitTime
      : b.lastVisitTime - a.lastVisitTime;
  });
}
function render() {
  const root = document.getElementById("history");
  root.textContent = "";
  visitDirection.style.display = sortMode.value === "most_visited" ? "" : "none";
  if (!entries.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No browsing history yet.";
    root.appendChild(empty);
    return;
  }
  const grouped = new Map();
  for (const item of entries) {
    const date = new Date((item.lastVisitTime || 0) * 1000);
    const year = date.getFullYear();
    const month = date.getMonth();
    const dayKey = `${year}-${month}-${date.getDate()}`;
    if (!grouped.has(year)) grouped.set(year, new Map());
    if (!grouped.get(year).has(month)) grouped.get(year).set(month, new Map());
    if (!grouped.get(year).get(month).has(dayKey)) grouped.get(year).get(month).set(dayKey, { date, items: [] });
    grouped.get(year).get(month).get(dayKey).items.push(item);
  }
  const groupDir = sortMode.value === "oldest" ? 1 : -1;
  for (const [year, months] of [...grouped.entries()].sort((a, b) => (a[0] - b[0]) * groupDir)) {
    const yearEl = document.createElement("section");
    yearEl.className = "year";
    yearEl.innerHTML = `<h2 class="year-title">${year}</h2>`;
    for (const [month, days] of [...months.entries()].sort((a, b) => (a[0] - b[0]) * groupDir)) {
      const monthEl = document.createElement("section");
      monthEl.className = "month";
      monthEl.innerHTML = `<h3 class="month-title">${monthLabel(year, month)}</h3>`;
      for (const day of [...days.values()].sort((a, b) => (a.date - b.date) * groupDir)) {
        const dayEl = document.createElement("section");
        dayEl.className = "day";
        const total = day.items.reduce((sum, item) => sum + item.visitCount, 0);
        const head = document.createElement("div");
        head.className = "day-head";
        head.innerHTML = `<div class="day-name"><span class="dot"></span>${dayLabel(day.date)}</div><div class="day-count">${visitText(total)}</div>`;
        dayEl.appendChild(head);
        for (const item of sortItems(day.items)) {
          const row = document.createElement("button");
          row.className = "item";
          row.type = "button";
          row.innerHTML = `<div><div class="title"></div><div class="url"></div></div><div class="meta">${visitText(item.visitCount)}</div>`;
          row.querySelector(".title").textContent = itemTitle(item);
          row.querySelector(".url").textContent = cleanUrl(item.url);
          row.onclick = () => post("history:open:" + encodeURIComponent(item.url));
          dayEl.appendChild(row);
        }
        monthEl.appendChild(dayEl);
      }
      yearEl.appendChild(monthEl);
    }
    root.appendChild(yearEl);
  }
}
sortMode.onchange = () => {
  render();
  post("history:sort:" + sortMode.value);
};
visitDirection.onchange = () => {
  render();
  post("history:visit-direction:" + visitDirection.value);
};
render();
</script>
</body>
</html>"#;
    template
        .replace("__ACCENT__", &accent)
        .replace("__DOMINANT__", &dominant)
        .replace("__SECONDARY__", &secondary)
        .replace("__ENTRIES__", &entries_json)
        .replace("__SORT_MODE__", sort_mode.as_str())
        .replace("__VISIT_DIRECTION__", visit_direction)
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

fn colorref_to_css(color: u32) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        color & 0xff,
        (color >> 8) & 0xff,
        (color >> 16) & 0xff
    )
}

fn parse_css_color_to_colorref(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    let r = (rgb >> 16) & 0xff;
    let g = (rgb >> 8) & 0xff;
    let b = rgb & 0xff;
    Some(r | (g << 8) | (b << 16))
}

unsafe fn take_pwstr(value: PWSTR) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *value.0.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(value.0, len);
    let out = String::from_utf16_lossy(slice);
    CoTaskMemFree(Some(value.0 as *const _));
    out
}

fn extension_info(extension: &ICoreWebView2BrowserExtension) -> BrowserExtensionInfo {
    unsafe {
        let mut id = PWSTR::null();
        let mut name = PWSTR::null();
        let mut enabled = BOOL::from(false);
        let id = if extension.Id(&mut id).is_ok() {
            take_pwstr(id)
        } else {
            String::new()
        };
        let name = if extension.Name(&mut name).is_ok() {
            take_pwstr(name)
        } else {
            String::new()
        };
        let _ = extension.IsEnabled(&mut enabled);
        BrowserExtensionInfo {
            id,
            name: if name.trim().is_empty() {
                "Unnamed extension".to_string()
            } else {
                name
            },
            enabled: enabled.as_bool(),
        }
    }
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
    if value.eq_ignore_ascii_case(":settings") || value.eq_ignore_ascii_case("aster:settings") {
        return "aster:settings".to_string();
    }
    if value.eq_ignore_ascii_case(":history") || value.eq_ignore_ascii_case("aster:history") {
        return "aster:history".to_string();
    }
    if value.eq_ignore_ascii_case(":extensions")
        || value.eq_ignore_ascii_case("aster:extensions")
    {
        return "aster:extensions".to_string();
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
            if let Some(h1) = chars.next() {
                hex.push(h1);
            }
            if let Some(h2) = chars.next() {
                hex.push(h2);
            }
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

fn serialize_tag_list(tags: &[String]) -> String {
    tags.iter()
        .map(|tag| escape_state(tag))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_tag_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(unescape_state)
        .filter(|tag| !tag.trim().is_empty())
        .collect()
}

fn js_string_literal(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn build_find_script(query: &str, delta: i32, highlight_color: String) -> String {
    format!(
        r##"(function(query, delta) {{
  const cls = "aster-find-mark";
  const activeCls = "aster-find-active";
  function clearMarks() {{
    document.querySelectorAll("mark." + cls).forEach((mark) => {{
      const parent = mark.parentNode;
      if (!parent) return;
      parent.replaceChild(document.createTextNode(mark.textContent || ""), mark);
      parent.normalize();
    }});
  }}
  function collectTextNodes(root) {{
    const nodes = [];
    if (!root) return nodes;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {{
      acceptNode(node) {{
        const parent = node.parentElement;
        if (!parent) return NodeFilter.FILTER_REJECT;
        const tag = parent.tagName;
        if (["SCRIPT","STYLE","NOSCRIPT","TEXTAREA","INPUT","MARK"].includes(tag)) {{
          return NodeFilter.FILTER_REJECT;
        }}
        return node.nodeValue && node.nodeValue.trim()
          ? NodeFilter.FILTER_ACCEPT
          : NodeFilter.FILTER_REJECT;
      }}
    }});
    while (walker.nextNode()) nodes.push(walker.currentNode);
    return nodes;
  }}
  query = (query || "").trim();
  if (!query) {{
    clearMarks();
    window.__asterFind = {{ query: "", index: 0 }};
    return {{ count: 0, index: 0 }};
  }}
  if (!window.__asterFind || window.__asterFind.query !== query) {{
    clearMarks();
    const needle = query.toLowerCase();
    collectTextNodes(document.body).forEach((node) => {{
      const text = node.nodeValue || "";
      const lower = text.toLowerCase();
      let cursor = 0;
      let hit = lower.indexOf(needle);
      if (hit < 0) return;
      const frag = document.createDocumentFragment();
      while (hit >= 0) {{
        if (hit > cursor) frag.appendChild(document.createTextNode(text.slice(cursor, hit)));
        const mark = document.createElement("mark");
        mark.className = cls;
        mark.textContent = text.slice(hit, hit + query.length);
        mark.style.background = {};
        mark.style.color = "#ffffff";
        mark.style.borderRadius = "2px";
        mark.style.padding = "0 1px";
        frag.appendChild(mark);
        cursor = hit + query.length;
        hit = lower.indexOf(needle, cursor);
      }}
      if (cursor < text.length) frag.appendChild(document.createTextNode(text.slice(cursor)));
      node.parentNode.replaceChild(frag, node);
    }});
    window.__asterFind = {{ query, index: 0 }};
  }}
  const marks = Array.from(document.querySelectorAll("mark." + cls));
  if (!marks.length) return {{ count: 0, index: 0 }};
  let index = window.__asterFind.index || 0;
  if (delta) index = (index + delta + marks.length) % marks.length;
  window.__asterFind.index = index;
  marks.forEach((mark) => {{
    mark.classList.remove(activeCls);
    mark.style.outline = "";
  }});
  const active = marks[index];
  active.classList.add(activeCls);
  active.style.outline = "2px solid #ffffff";
  active.scrollIntoView({{ block: "center", inline: "nearest" }});
  return {{ count: marks.length, index }};
}})({}, {});"##,
        js_string_literal(&highlight_color),
        js_string_literal(query),
        delta
    )
}

fn parse_json_usize_field(raw: &str, field: &str) -> Option<usize> {
    let needle = format!("\"{}\":", field);
    let start = raw.find(&needle)? + needle.len();
    let mut digits = String::new();
    for ch in raw[start..].chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        } else if ch != ' ' {
            break;
        }
    }
    digits.parse::<usize>().ok()
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

struct BrushCache {
    brushes: HashMap<u32, HBRUSH>,
}

impl Drop for BrushCache {
    fn drop(&mut self) {
        unsafe {
            for (_, b) in self.brushes.drain() {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
        }
    }
}

thread_local! {
    static BRUSH_CACHE: RefCell<BrushCache> = RefCell::new(BrushCache {
        brushes: HashMap::new(),
    });
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

fn previous_root_row(
    rows: &[(SidebarRow, RECT)],
    before_index: usize,
    pinned: bool,
) -> Option<SidebarRow> {
    rows.iter()
        .take(before_index)
        .rev()
        .take_while(|(row, _)| pinned || !matches!(row, SidebarRow::Label(SidebarLabel::Tabs)))
        .find_map(|(row, _)| match *row {
            SidebarRow::Folder(folder_id) => Some(SidebarRow::Folder(folder_id)),
            SidebarRow::SplitGroup(group_id) => Some(SidebarRow::SplitGroup(group_id)),
            SidebarRow::Tab(tab_index) => Some(SidebarRow::Tab(tab_index)),
            SidebarRow::TabGhost(_) | SidebarRow::Label(_) => None,
        })
}

fn with_app<F>(hwnd: HWND, f: F)
where
    F: FnOnce(&mut App),
{
    let re_entered = WITH_APP_GUARD.with(|guard| guard.replace(true));
    if re_entered {
        return;
    }
    unsafe {
        let ptr = WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !ptr.is_null() {
            f(&mut *ptr);
        }
    }
    WITH_APP_GUARD.with(|guard| guard.set(false));
}

fn with_app_return<T, F>(hwnd: HWND, f: F) -> Option<T>
where
    F: FnOnce(&mut App) -> T,
{
    unsafe {
        let re_entered = WITH_APP_GUARD.with(|g| g.replace(true));
        if re_entered {
            return None;
        }
        let ptr = WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        let result = if ptr.is_null() {
            None
        } else {
            Some(f(&mut *ptr))
        };
        WITH_APP_GUARD.with(|g| g.set(false));
        result
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
    clean_all_prefixes(url)
        .to_ascii_lowercase()
        .starts_with(&query)
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

const CREATE_NO_WINDOW: u32 = 0x08000000;

fn is_aster_default_browser() -> bool {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("reg");
    cmd.arg("query")
       .arg("HKCU\\Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\https\\UserChoice")
       .arg("/v")
       .arg("ProgId");
    cmd.creation_flags(CREATE_NO_WINDOW);

    if let Ok(output) = cmd.output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains("AsterHTML")
    } else {
        false
    }
}

fn make_aster_default_browser() {
    use std::os::windows::process::CommandExt;
    if let Ok(exe_path) = std::env::current_exe() {
        let exe_str = exe_path.to_string_lossy();

        let keys = vec![
            (
                "HKCU\\Software\\Classes\\AsterHTML".to_string(),
                "".to_string(),
                "REG_SZ".to_string(),
                "Aster HTML Document".to_string(),
            ),
            (
                "HKCU\\Software\\Classes\\AsterHTML".to_string(),
                "URL Protocol".to_string(),
                "REG_SZ".to_string(),
                "".to_string(),
            ),
            (
                "HKCU\\Software\\Classes\\AsterHTML\\DefaultIcon".to_string(),
                "".to_string(),
                "REG_SZ".to_string(),
                format!("{},0", exe_str),
            ),
            (
                "HKCU\\Software\\Classes\\AsterHTML\\shell\\open\\command".to_string(),
                "".to_string(),
                "REG_SZ".to_string(),
                format!("\"{}\" \"%1\"", exe_str),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster".to_string(),
                "".to_string(),
                "REG_SZ".to_string(),
                "Aster".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities".to_string(),
                "ApplicationDescription".to_string(),
                "REG_SZ".to_string(),
                "Aster Web Browser".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities".to_string(),
                "ApplicationIcon".to_string(),
                "REG_SZ".to_string(),
                format!("{},0", exe_str),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities".to_string(),
                "ApplicationName".to_string(),
                "REG_SZ".to_string(),
                "Aster".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities\\FileAssociations"
                    .to_string(),
                ".htm".to_string(),
                "REG_SZ".to_string(),
                "AsterHTML".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities\\FileAssociations"
                    .to_string(),
                ".html".to_string(),
                "REG_SZ".to_string(),
                "AsterHTML".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities\\URLAssociations"
                    .to_string(),
                "http".to_string(),
                "REG_SZ".to_string(),
                "AsterHTML".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\Capabilities\\URLAssociations"
                    .to_string(),
                "https".to_string(),
                "REG_SZ".to_string(),
                "AsterHTML".to_string(),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\DefaultIcon".to_string(),
                "".to_string(),
                "REG_SZ".to_string(),
                format!("{},0", exe_str),
            ),
            (
                "HKCU\\Software\\Clients\\StartMenuInternet\\Aster\\shell\\open\\command"
                    .to_string(),
                "".to_string(),
                "REG_SZ".to_string(),
                format!("\"{}\"", exe_str),
            ),
            (
                "HKCU\\Software\\RegisteredApplications".to_string(),
                "Aster".to_string(),
                "REG_SZ".to_string(),
                "Software\\Clients\\StartMenuInternet\\Aster\\Capabilities".to_string(),
            ),
        ];

        for (key, val_name, val_type, val_data) in keys {
            let mut cmd = std::process::Command::new("reg");
            cmd.arg("add").arg(&key);
            if !val_name.is_empty() {
                cmd.arg("/v").arg(&val_name);
            } else {
                cmd.arg("/ve");
            }
            cmd.arg("/t")
                .arg(&val_type)
                .arg("/d")
                .arg(&val_data)
                .arg("/f");
            cmd.creation_flags(CREATE_NO_WINDOW);
            let _ = cmd.output();
        }

        let _ = std::process::Command::new("cmd")
            .args(&[
                "/c",
                "start",
                "ms-settings:defaultapps?registeredAppUser=Aster",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
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
        assert_eq!(
            normalize_url_for_dedup("https://www.google.com/"),
            "google.com"
        );
        assert_eq!(
            normalize_url_for_dedup("  https://www.google.com/search?q=foo/ "),
            "google.com/search?q=foo"
        );
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

    #[test]
    fn test_collect_bookmarks_from_json() {
        let raw = serde_json::json!({
            "roots": {
                "bookmark_bar": {
                    "name": "Bookmarks bar",
                    "children": [
                        { "type": "url", "name": "Aster", "url": "https://aster.example" }
                    ]
                }
            }
        });
        let mut bookmarks = Vec::new();
        collect_bookmarks_from_json(&raw, "", &mut bookmarks);
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].title, "Aster");
        assert_eq!(bookmarks[0].folder, "Bookmarks bar");
    }

    #[test]
    fn test_parse_bookmark_html() {
        let raw = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1><DL><p><DT><A HREF="https://example.com?a=1&amp;b=2">Example &amp; Co</A></DL><p>"#;
        let bookmarks = parse_bookmark_html(raw);
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].url, "https://example.com?a=1&b=2");
        assert_eq!(bookmarks[0].title, "Example & Co");
    }

    #[test]
    fn test_chrome_time_to_unix_secs() {
        let unix = 1_700_000_000u64;
        let chrome = 11_644_473_600_i64 * 1_000_000 + unix as i64 * 1_000_000;
        assert_eq!(chrome_time_to_unix_secs(chrome), unix);
    }

    #[test]
    fn test_favicon_discovery_helpers() {
        let html = r#"
            <html><head>
              <link rel="preload" href="/not-icon.png">
              <link rel="shortcut icon" href="/favicon-32.png">
              <link href='touch.png' rel='apple-touch-icon'>
            </head></html>
        "#;
        let hrefs = extract_favicon_hrefs(html);
        assert_eq!(hrefs, vec!["/favicon-32.png", "touch.png"]);
        assert_eq!(
            resolve_url("https://example.com/docs/page", "/favicon-32.png"),
            Some("https://example.com/favicon-32.png".to_string())
        );
        assert_eq!(
            resolve_url("https://example.com/docs/page", "touch.png"),
            Some("https://example.com/docs/touch.png".to_string())
        );
        assert_eq!(
            favicon_candidate_urls("https://example.com/docs/page", None),
            vec!["https://example.com/favicon.ico".to_string()]
        );
    }
}
