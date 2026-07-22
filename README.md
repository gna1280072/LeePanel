# LeePanel

<p align="center"><img src="public/app-icon.png" alt="LeePanel Logo" width="128" height="128"></p>

<p align="center">
  <a href="https://github.com/gna1280072/LeePanel/stargazers"><img src="https://img.shields.io/github/stars/gna1280072/LeePanel?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/gna1280072/LeePanel/releases"><img src="https://img.shields.io/github/v/release/gna1280072/LeePanel?style=flat-square" alt="Release"></a>
  <a href="https://github.com/gna1280072/LeePanel/releases"><img src="https://img.shields.io/github/downloads/gna1280072/LeePanel/total?style=flat-square" alt="Downloads"></a>
  <a href="https://github.com/gna1280072/LeePanel/blob/main/LICENSE"><img src="https://img.shields.io/github/license/gna1280072/LeePanel?style=flat-square" alt="License"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Tauri-2.x-FFC131?style=flat-square&logo=tauri" alt="Tauri">
  <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react" alt="React">
  <img src="https://img.shields.io/badge/TypeScript-6.0-3178C6?style=flat-square&logo=typescript" alt="TypeScript">
  <img src="https://img.shields.io/badge/Rust-stable-DEA584?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Vite-8-646CFF?style=flat-square&logo=vite" alt="Vite">
  <img src="https://img.shields.io/badge/Server-Ubuntu%20%7C%20Debian-E95420?style=flat-square&logo=ubuntu" alt="Server">
</p>

[中文文档](README.zh-CN.md) | [ Download](https://github.com/gna1280072/LeePanel/releases)

LeePanel — Free and open-source, the next-generation Linux server management panel.

Traditional Linux/VPS management panels frequently suffer from security vulnerabilities, causing endless headaches for server administrators.

**LeePanel was built to solve this once and for all.**

> 🛡️ **Zero server-side code** — All operations are performed via SSH commands from your local machine. **No** panel code installed on the server, **no** extra ports exposed, eliminating the panel's own security risks at the root.

A lightweight cross-platform desktop app built with Tauri 2 + React, providing a single native client to manage:

- 🔌 SSH Terminal & 📁 SFTP File Management
- 🌐 Nginx / ️ MySQL/MariaDB / ️ PHP /  Redis / 🐳 Docker
- ️ Firewall / 🔒 Free SSL Certificates

Installer as small as **6 MB** — lightweight and flexible! Completely replacing traditional browser-based panels.

If you have any suggestions or feedback, feel free to share them in [GitHub Discussions](https://github.com/gna1280072/LeePanel/discussions).

🌐 Website: https://www.LeePanel.com

 
## 💡 Why LeePanel?

| Dimension | Traditional Web Panel ❌ | LeePanel ✅ |
|-----------|--------------------------|-------------|
| Deployment | Installs and runs panel code on the server, consuming resources | Runs entirely on your desktop, nothing installed on the server |
| Port Exposure | Opens ports 8888/8080 to the internet | Only your existing SSH port is used |
| Attack Surface | Panel itself becomes the #1 attack target | Server stays exactly as you configured it |
| Security Risk | Panel runs as root, vulnerabilities = full server compromise | Does not intervene with the server, no such risk |
| Uninstall | Requires SSH into server to run uninstall script, hard to clean up | Just close the app, zero residue |
| Security Index | ⭐⭐ | ⭐⭐⭐⭐⭐ |

##  System Requirements

| Platform | Version |
|----------|--------|
| Windows | 10/11 (64-bit) |
| macOS | 12+ (Intel / Apple Silicon) |
| Linux | x64 / arm64 (AppImage) |

## 📦 Installer Size

| Platform | Size |
|----------|------|
| Windows | ~6 MB |
| macOS | ~6 MB |
| Linux | ~6 MB |

## 🖥️ Supported Servers
> 🖥️ All features are currently tested on **Ubuntu** / **Debian**. More distributions coming soon, stay tuned...

##  Preview

<p align="center">
  <img src="public/img/01.png" alt="Preview 1" width="800">
</p>

<p align="center">
  <img src="public/img/02.png" alt="Preview 2" width="800">
</p>

<p align="center">
  <img src="public/img/03.png" alt="Preview 3" width="800">
</p>

## ⚡ Features

### 🔌 Connection & Terminal
- 🔑 SSH password / key authentication with credential storage
- ️ Full-featured xterm.js terminal with clipboard, web links, and search
-  Auto-reconnect on connection loss
-  Multi-server concurrent management — each session operates independently without blocking others

###  File Management
- 🌐 Remote file browser with drag-and-drop upload/download
-  Chunked upload for large files with progress tracking
- ️ Compress / extract archives (zip, tar.gz)
- 🔐 File permission management (chmod)
- 📋 Batch file operations (copy, move, delete, rename)
- ⭐ Favorites for quick access
-  Directory caching for faster navigation

### 🏗️ LNMP Stack Management
-  One-click Nginx, MySQL/MariaDB, PHP-FPM installation with version selection
- ️ Service start / stop / restart / reload controls
-  Real-time status monitoring
- 🔀 PHP multi-version management and switching

### 🌐 Site Management
- 🏠 Nginx virtual host creation and configuration
- 🔒 Let's Encrypt SSL certificate management
- 🔁 Reverse proxy setup with WebSocket support
- ️ Hotlink protection configuration
-  PHP version switching per site
- 📝 Rewrite rules management
- ️ Site enable / disable / delete

### ️ Database Management
- 📊 MySQL/MariaDB database CRUD operations
- 👥 User permission management (localhost / any host / specific IP)
-  Database backup and restore (zip format)
-  SQL file import from editor or backup
-  Root password management
- 📝 Database remarks for team documentation

### ⚡ Redis Management
- 🔍 Key browsing with SCAN-based pagination
-  Key CRUD operations with TTL support
- 🗂️ Multi-database (DB0–DB15) switching
- 🧹 Database flush with confirmation
-  Backup and restore

### 🐳 Docker Management
- 🔄 Container lifecycle management (start, stop, restart, remove)
- 📦 Image management (pull, remove, run)
- 📋 Container logs viewer
- 🔗 Docker mirror source configuration
-  One-click Docker installation with multiple mirror options

### 🖧 System & Network
- 📊 Real-time CPU, memory, disk, and network monitoring
- 📋 Process list with resource usage
- ️ Firewall rule management (ufw / firewalld / iptables)
- 🚀 BBR TCP congestion control
- 🖥️ System information dashboard
- ⏱️ Server uptime tracking

### 📦 Software Repository
-  Package manager integration (apt)
- 🔗 Custom software source management
- 🔄 Software install / uninstall / update
- 🔍 Available PHP version browsing

### ⚙️ Server Settings
-  SSH authentication mode (password / key) management
-  SSH key generation (RSA, Ed25519, ECDSA) and deployment
- 🔄 Server reboot (normal / force)
- 🗂️ File cache management

### 🌍 Internationalization
- 🌐 English and Simplified Chinese support
- 💾 Language preference persisted locally
-  In-app language switcher

## ️ Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop Framework | Tauri 2.x |
| Frontend | React 19 + TypeScript |
| Build Tool | Vite 8 |
| Terminal | xterm.js 6 |
| SSH Client | russh (Rust) |
| SFTP | russh-sftp (Rust) |
| Storage | SQLite (rusqlite) |
| i18n | react-i18next |

## 🚀 Development

### 📋 Prerequisites
- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://www.rust-lang.org/tools/install)
- System dependencies required by [Tauri](https://v2.tauri.app/start/prerequisites/)

### 🏃 Setup

```bash
# Install dependencies
npm install

# Start development server
npm run tauri dev

# Build for production
npm run tauri build
```

##  License

MIT License. See [LICENSE](LICENSE) for details.