# LeePanel

A cross-platform desktop application for SSH server management, built with Tauri 2 and React. LeePanel provides an all-in-one remote management experience for DevOps engineers and developers.

[中文文档](README.zh-CN.md)

## Features

### Connection & Terminal
- SSH password / key authentication
- Full-featured xterm.js terminal with clipboard and web links support
- Auto-reconnect on connection loss

### File Management
- Remote file browser with drag-and-drop upload/download
- Compress / extract archives (zip, tar.gz)
- File permission management
- Favorites for quick access

### LNMP Stack Management
- One-click Nginx, MySQL/MariaDB, PHP-FPM installation
- Service start / stop / restart / reload controls
- Real-time status monitoring

### Site Management
- Nginx virtual host creation and configuration
- Let's Encrypt SSL certificate management
- Reverse proxy setup with WebSocket support
- Hotlink protection
- PHP version switching per site
- Rewrite rules management

### Database Management
- MySQL/MariaDB database CRUD operations
- User permission management (localhost / any host / specific IP)
- Database backup and restore (zip format)
- SQL file import
- Root password management

### Redis Management
- Key browsing with SCAN-based pagination
- Key CRUD operations with TTL support
- Multi-database (DB0–DB15) switching
- Database flush with confirmation
- Backup and restore

### System & Network
- Real-time CPU, memory, disk, and network monitoring
- Process list with resource usage
- Firewall rule management (ufw / firewalld / iptables)
- BBR TCP congestion control
- System information dashboard

### Server Settings
- Auto-reconnect configuration
- File cache management
- Software update checker
- Server reboot (normal / force)
- SSH authentication mode (password / key) management
- SSH key generation and deployment

### Internationalization
- English and Simplified Chinese support
- Language preference persisted locally
- In-app language switcher

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop Framework | Tauri 2.x |
| Frontend | React 19 + TypeScript |
| Build Tool | Vite 8 |
| Terminal | xterm.js 6 |
| SSH Client | russh (Rust) |
| Storage | SQLite (rusqlite) |
| i18n | react-i18next |

## Development

### Prerequisites
- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://www.rust-lang.org/tools/install)
- System dependencies required by [Tauri](https://v2.tauri.app/start/prerequisites/)

### Setup

```bash
# Install dependencies
npm install

# Start development server
npm run tauri dev

# Build for production
npm run tauri build
```

## Project Structure

```
src/
├── App.tsx                  # Root component
├── main.tsx                 # Entry point
├── i18n/
│   ├── index.ts             # i18next initialization
│   ├── en.json              # English translations
│   └── zh-CN.json           # Chinese translations
├── components/
│   ├── Sidebar.tsx          # Server list + language switcher
│   ├── ServerPanel.tsx      # Navigation bar
│   ├── FileBrowser.tsx      # Remote file manager
│   ├── Terminal.tsx         # xterm.js terminal
│   └── panels/
│       ├── Dashboard.tsx    # System overview
│       ├── DatabasePanel.tsx
│       ├── RedisPanel.tsx
│       ├── SitesPanel.tsx
│       ├── EditSite.tsx
│       ├── NginxPanel.tsx
│       ├── MonitorPanel.tsx
│       ├── FirewallPanel.tsx
│       ├── SslPanel.tsx
│       ├── BbrPanel.tsx
│       ├── InstallLnmp.tsx
│       ├── SoftwareRepo.tsx
│       ├── SiteLogsPanel.tsx
│       └── ServerSettingsPanel.tsx
src-tauri/
├── src/                     # Rust backend (SSH, SQLite, system commands)
└── tauri.conf.json          # Tauri configuration
```

## License

Private — All rights reserved.
