# 🚀 FlashTransfer

> **A modern, fast, and secure cross-platform file transfer application built with Tauri and React.**

FlashTransfer is designed to transfer files directly between nearby devices over a local network with high speed, strong reliability, resumable transfers, file-integrity verification, secure pairing, and multi-device support.

---

## ✨ Features
- **High-Speed Transfers**: Direct P2P file transfers over a local network.
- **Resumable Transfers**: Pause, resume, and recover failed transfers seamlessly.
- **Secure Pairing**: Device verification and secure communication channels.
- **Multi-Device Support**: Connect and transfer files across multiple devices simultaneously.
- **File-Integrity**: Verifies file integrity using BLAKE3 hashing.

## 🛠️ Architecture

FlashTransfer follows a clean architecture with strong separation of concerns, built using modern and powerful technologies.

### Technology Stack
- 🎨 **Frontend**: React + TypeScript + Vite
- 🖥️ **Desktop Framework**: Tauri
- ⚙️ **Backend / Core Engine**: Rust
- 🗄️ **Data / Storage**: SQLite (for metadata only, not file content)

### Components Overview
- **React UI**: Handles presentation, UI state, user interactions, and displaying transfer state. It communicates with the backend via Tauri commands and events.
- **Rust Core**: Responsible for device discovery, network communication, TCP/UDP/mDNS, session management, file chunking, hashing (BLAKE3), encryption, parallel transfers, and storage.
- **Tauri Communication**: React and Rust communicate via commands and events. React invokes Tauri commands (e.g., `start_transfer`), and Rust emits events (e.g., `transfer-progress`). Event payloads are strongly typed in both Rust and TypeScript.

---

## 💻 Development Guide

### Environment Setup
Ensure you have the following installed before starting development:
- Node.js (v20+) and npm
- Rust and Cargo (via [rustup](https://rustup.rs/))
- Tauri build prerequisites (e.g. MSVC C++ Build Tools on Windows)

### Project Structure
- `frontend/` - Contains the React + Vite application.
- `src-tauri/` - Contains the Rust core and Tauri application shell.

### Running the Application

The Tauri CLI is installed as a local npm devDependency in the `frontend/` folder.
Run the following from the **project root**:

```bash
# From the project root (e.g., d:\Personal Project\FlashTransfer)
.\frontend\node_modules\.bin\tauri dev
```
*(The dev command starts the Vite frontend dev server, compiles the Rust backend, and opens the Tauri desktop window.)*

**Note:** 
- Do NOT run `cargo tauri dev` — `tauri` is not a Cargo subcommand.
- Do NOT run from inside the `frontend/` directory — the Tauri CLI must be able to find `src-tauri/tauri.conf.json`, which requires running from the root.

### Code Quality
- **Frontend**: Use `npm run lint --prefix frontend` to lint. Use `npm run format --prefix frontend` for formatting.
- **Backend**: Use `cargo fmt` to format Rust code, and `cargo clippy` for linting (run inside `src-tauri/`).

---

### 👨‍💻 Developed By
**Badhon Saha**
