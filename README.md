# Aster

Aster is a lightweight Windows desktop browser built in Rust with the native Win32 API and Microsoft Edge WebView2. It uses the Chromium rendering engine through the installed WebView2 runtime and does not use Electron.

## Features

- Chromium-powered browsing through WebView2
- Vertical tab bar inspired by Arc and Zen
- Multiple tabs with close and switch actions
- Back, forward, reload, and address navigation
- Vercel-black visual theme
- Keyboard shortcuts: `Ctrl+L`, `Ctrl+T`, `Ctrl+W`, `Alt+Left`, `Alt+Right`, `F5`

## Build

```powershell
cargo build --release
```

The executable is produced at:

```text
target\release\aster.exe
```

WebView2 Runtime is required. It is included by default on modern Windows 11 systems and many Windows 10 systems.
