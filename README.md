# LeePanel

<p align="center"><img src="public/app-icon.png" alt="LeePanel Logo" width="128" height="128"></p>

[中文文档](README.zh-CN.md) | [download软件下载](https://github.com/gna1280072/LeePanel/releases)

LeePanel — Free and open-source, the NEXT-generation Linux server management panel.

Most popular server panels today are installed directly on the server, and these panels themselves frequently suffer from security vulnerabilities, causing endless headaches for server administrators worldwide.

We built LeePanel to solve this problem once and for all.

What makes us different: all operations are performed by sending SSH commands from your local machine to the server. Not a single line of panel code needs to be installed on the server — making it fundamentally more secure!

A lightweight cross-platform desktop app built with Tauri 2 + React, replacing traditional browser-based panels.

Manage SSH connections, files (SFTP), Nginx, MySQL/MariaDB, PHP, Redis, Docker, firewalls, SSL certificates and more — all from a single native desktop client...

Released on July 18, 2026, LeePanel is still in its early stages. We've currently tested and verified compatibility with mainstream Ubuntu/Debian versions, with support for more Linux distributions in progress.

If you have any suggestions or feedback during use, feel free to share them in GitHub Discussions.

Website: https://www.LeePanel.com

 
## Features

### Connection & Terminal
- SSH password / key authentication with credential storage
- Full-featured xterm.js terminal with clipboard, web links, and search
- Auto-reconnect on connection loss
- Multi-server concurrent management — each session operates independently without blocking others

### File Management
- Remote file browser with drag-and-drop upload/download
- Chunked upload for large files with progress tracking
- Compress / extract archives (zip, tar.gz)
- File permission management (chmod)
- Batch file operations (copy, move, delete, rename)
- Favorites for quick access
- Directory caching for faster navigation

### LNMP Stack Management
- One-click Nginx, MySQL/MariaDB, PHP-FPM installation with version selection
- Service start / stop / restart / reload controls
- Real-time status monitoring
- PHP multi-version management and switching

### Site Management
- Nginx virtual host creation and configuration
- Let's Encrypt SSL certificate management
- Reverse proxy setup with WebSocket support
- Hotlink protection configuration
- PHP version switching per site
- Rewrite rules management
- Site enable / disable / delete

### Database Management
- MySQL/MariaDB database CRUD operations
- User permission management (localhost / any host / specific IP)
- Database backup and restore (zip format)
- SQL file import from editor or backup
- Root password management
- Database remarks for team documentation

### Redis Management
- Key browsing with SCAN-based pagination
- Key CRUD operations with TTL support
- Multi-database (DB0–DB15) switching
- Database flush with confirmation
- Backup and restore

### Docker Management
- Container lifecycle management (start, stop, restart, remove)
- Image management (pull, remove, run)
- Container logs viewer
- Docker mirror source configuration
- One-click Docker installation with multiple mirror options

### System & Network
- Real-time CPU, memory, disk, and network monitoring
- Process list with resource usage
- Firewall rule management (ufw / firewalld / iptables)
- BBR TCP congestion control
- System information dashboard
- Server uptime tracking

### Software Repository
- Package manager integration (apt)
- Custom software source management
- Software install / uninstall / update
- Available PHP version browsing

### Server Settings
- SSH authentication mode (password / key) management
- SSH key generation (RSA, Ed25519, ECDSA) and deployment
- Server reboot (normal / force)
- File cache management

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
| SFTP | russh-sftp (Rust) |
| Storage | SQLite (rusqlite) |
| i18n | react-i18next |

## Architecture

### Backend (Rust)

```
src-tauri/src/
├── lib.rs       # Tauri command handlers (bridge between frontend and backend)
├── ssh.rs       # SSH session management, SshCache, connection lifecycle
├── server.rs    # Server operations (LNMP, sites, databases, Docker, etc.)
├── config.rs    # Configuration management
├── db.rs        # SQLite database operations (favorites, cache, metadata)
└── main.rs      # Entry point
```

**SSH Session Architecture:**
- Each SSH session is independently managed via `SshSession` (Clone + Arc-wrapped)
- Session storage uses `std::sync::RwLock` for lock-free concurrent reads
- `SshCache` (in-memory, sync `std::sync::Mutex`) stores SSH response data with TTL
- All server commands release the global manager lock before network operations, ensuring one slow server never blocks operations on other servers

### Frontend (React + TypeScript)

```
src/
├── App.tsx              # Root component with tab routing
├── main.tsx             # Entry point
├── i18n/                # Internationalization
│   ├── index.ts         # i18next initialization
│   ├── en.json          # English translations
│   └── zh-CN.json       # Chinese translations
├── components/
│   ├── Sidebar.tsx      # Server list + connection manager
│   ├── ServerPanel.tsx  # Tab navigation bar
│   ├── FileBrowser.tsx  # Remote file manager with SFTP
│   ├── Terminal.tsx     # xterm.js terminal wrapper
│   └── panels/
│       ├── Dashboard.tsx         # System overview
│       ├── DatabasePanel.tsx     # MySQL/MariaDB management
│       ├── RedisPanel.tsx        # Redis management
│       ├── DockerPanel.tsx       # Docker container & image management
│       ├── SitesPanel.tsx        # Site list
│       ├── EditSite.tsx          # Site editor (Nginx config, SSL, proxy)
│       ├── NginxPanel.tsx        # Nginx configuration
│       ├── PhpPanel.tsx          # PHP version management
│       ├── InstallLnmp.tsx       # LNMP stack installer
│       ├── MonitorPanel.tsx      # System monitoring
│       ├── FirewallPanel.tsx     # Firewall rules
│       ├── SslPanel.tsx          # SSL certificate management
│       ├── BbrPanel.tsx          # BBR congestion control
│       ├── SoftwareRepo.tsx      # Software package management
│       ├── SiteLogsPanel.tsx     # Site log viewer
│       └── ServerSettingsPanel.tsx # Server settings
```

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

## License

MIT License. See [LICENSE](LICENSE) for details.