<div align="center">
  <img src="assets/aster.ico" width="128" height="128" alt="Aster Logo">
  <h1>Aster Browser</h1>
  <p><strong>Our Mission:</strong> To build the best browser possible by taking all the good features and aspects of other browsers and adding our own unique innovations all while ensuring <strong>your data is 100% yours</strong>. Aster browser data is stored locally, no tracking, no telemetry, no data collection. Everything is stored directly on your machine. Period.</p>
</div>

---

## 🚀 Quick Installation

To install or update Aster automatically, open PowerShell and run this exact one-liner (**completely dependency-free — does NOT require Rust, Cargo, or Git**):

```powershell
irm https://raw.githubusercontent.com/ahyanistheEmty/Aster/main/install.ps1 | iex
```

---

## 🛠️ For Developers

Aster is a fast, Chromium-based desktop browser natively compiled in Rust via `webview2-com`. It acts as a lightweight standalone application that manages its own tabs, process states, and layout rendering directly through Win32 APIs, ensuring minimal overhead.

### Project Structure

```text
Aster/
├── assets/
│   └── aster.ico        # Standalone executable icon
├── src/
│   ├── main.rs          # Core application logic, Win32 bindings, WebView2 lifecycle
├── aster.rc             # Resource script for embedding the Windows icon
├── build.rs             # Build script for rasterizing icons into the PE header
├── Cargo.toml           # Rust dependencies and binary targets
└── install.ps1          # Automated interactive installation script
```

### Building from Source

To compile the standalone Windows executable yourself:

```powershell
# Clone the repository
git clone https://github.com/ahyanistheEmty/Aster.git
cd Aster

# Build the release executable
cargo build --release

# The compiled binary will be located at target/release/Aster.exe
```

---

## ✨ Features

- **Workspaces**: Organize your browser into dedicated, isolated contexts for work, personal, and research.
- **Folders**: Group tabs locally into folders to maintain a clean visual sidebar.
- **Vertical Tabs**: Side-docked tabs optimized for modern widescreen monitors.
- **Blazing Fast**: Built natively in Rust. Extremely lightweight with minimal memory overhead.

---

## 🤝 Contributing

Contributions, issues, and feature requests are welcome! Feel free to check the [issues page](https://github.com/ahyanistheEmty/Aster/issues).

## 📝 License

This project is [MIT](LICENSE) licensed.
