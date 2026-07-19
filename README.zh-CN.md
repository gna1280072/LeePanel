# LeePanel

[English](README.md) | [下载软件](https://github.com/gna1280072/LeePanel/releases)

LeePanel — 免费开源，下一代 Linux 服务器管理面板。

因目前流行的各种安装在服务器上的面板软件，这些面板自身经常发现安全问题，令广大的服务器管理员苦不堪言。

我们开发LeePanel，希望彻底解决这个问题。

我们的创新之处在于：所有操作通过本地电脑向服务器发送SSH命令进行，不需要在服务器上安装任何一行的面板代码，让服务器更安全！

基于 Tauri 2 + React 构建的轻量级跨平台桌面应用，取代传统浏览器面板。

通过单一原生客户端统一管理 SSH 连接、文件（SFTP）、Nginx、MySQL/MariaDB、PHP、Redis、Docker、防火墙、SSL 证书等功能.....

当前项目发布于2026年7月18日，还是个宝宝，我们当前在ubuntu/debian上主流版本测试通过，更多的LINUX版本正在适配中。

如果您在使用过程有任何建议和意见，欢迎到github Discussions 里提出。

官网: https://www.LeePanel.com
 

## 功能特性

### 连接与终端
- SSH 密码 / 密钥认证，支持凭证存储
- 基于 xterm.js 的全功能终端，支持剪贴板、超链接和搜索
- 断线自动重连
- 多服务器并发管理 — 每个会话独立运行，互不阻塞

### 文件管理
- 远程文件浏览器，支持拖拽上传/下载
- 大文件分块上传，带进度跟踪
- 压缩包管理（zip、tar.gz）
- 文件权限管理（chmod）
- 批量文件操作（复制、移动、删除、重命名）
- 收藏夹快速访问
- 目录缓存加速浏览

### LNMP 环境管理
- 一键安装 Nginx、MySQL/MariaDB、PHP-FPM，支持版本选择
- 服务启动 / 停止 / 重启 / 重载控制
- 实时状态监控
- PHP 多版本管理与切换

### 站点管理
- Nginx 虚拟主机创建与配置
- Let's Encrypt SSL 证书管理
- 反向代理配置（支持 WebSocket）
- 防盗链保护配置
- 站点级 PHP 版本切换
- Rewrite 规则管理
- 站点启用 / 禁用 / 删除

### 数据库管理
- MySQL/MariaDB 数据库增删改查
- 用户权限管理（localhost / 任意主机 / 指定 IP）
- 数据库备份与恢复（zip 格式）
- SQL 文件导入（编辑器或备份文件）
- root 密码管理
- 数据库备注（团队文档协作）

### Redis 管理
- 基于 SCAN 的 Key 浏览与分页
- Key 增删改查，支持 TTL
- 多数据库（DB0–DB15）切换
- 数据库清空确认
- 备份与恢复

### Docker 管理
- 容器生命周期管理（启动、停止、重启、删除）
- 镜像管理（拉取、删除、运行）
- 容器日志查看器
- Docker 镜像源配置
- 一键安装 Docker，支持多种镜像源

### 系统与网络
- CPU、内存、磁盘、网络实时监控
- 进程列表与资源占用
- 防火墙规则管理（ufw / firewalld / iptables）
- BBR TCP 拥塞控制
- 系统信息仪表盘
- 服务器运行时间追踪

### 软件仓库
- 包管理器集成（apt）
- 自定义软件源管理
- 软件安装 / 卸载 / 更新
- 可用 PHP 版本浏览

### 服务器设置
- SSH 认证模式（密码 / 密钥）管理
- SSH 密钥生成（RSA、Ed25519、ECDSA）与部署
- 服务器重启（正常 / 强制）
- 文件缓存管理

### 多语言支持
- 英文与简体中文
- 语言偏好本地持久化
- 应用内语言切换器

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri 2.x |
| 前端 | React 19 + TypeScript |
| 构建工具 | Vite 8 |
| 终端 | xterm.js 6 |
| SSH 客户端 | russh (Rust) |
| SFTP | russh-sftp (Rust) |
| 存储 | SQLite (rusqlite) |
| 国际化 | react-i18next |

## 架构

### 后端 (Rust)

```
src-tauri/src/
├── lib.rs       # Tauri 命令处理层（前端与后端的桥梁）
├── ssh.rs       # SSH 会话管理、SshCache、连接生命周期
├── server.rs    # 服务器操作（LNMP、站点、数据库、Docker 等）
├── config.rs    # 配置管理
├── db.rs        # SQLite 数据库操作（收藏、缓存、元数据）
└── main.rs      # 入口文件
```

**SSH 会话架构：**
- 每个 SSH 会话通过 `SshSession`（Clone + Arc 包装）独立管理
- 会话存储使用 `std::sync::RwLock` 实现无锁并发读取
- `SshCache`（内存缓存，同步 `std::sync::Mutex`）存储带 TTL 的 SSH 响应数据
- 所有服务器命令在网络操作前释放全局管理器锁，确保一台慢服务器不会阻塞其他服务器的操作

### 前端 (React + TypeScript)

```
src/
├── App.tsx              # 根组件（标签页路由）
├── main.tsx             # 入口文件
├── i18n/                # 国际化
│   ├── index.ts         # i18next 初始化
│   ├── en.json          # 英文翻译
│   └── zh-CN.json       # 中文翻译
├── components/
│   ├── Sidebar.tsx      # 服务器列表 + 连接管理器
│   ├── ServerPanel.tsx  # 标签导航栏
│   ├── FileBrowser.tsx  # SFTP 远程文件管理器
│   ├── Terminal.tsx     # xterm.js 终端封装
│   └── panels/
│       ├── Dashboard.tsx         # 系统概览
│       ├── DatabasePanel.tsx     # MySQL/MariaDB 管理
│       ├── RedisPanel.tsx        # Redis 管理
│       ├── DockerPanel.tsx       # Docker 容器与镜像管理
│       ├── SitesPanel.tsx        # 站点列表
│       ├── EditSite.tsx          # 站点编辑器（Nginx 配置、SSL、代理）
│       ├── NginxPanel.tsx        # Nginx 配置
│       ├── PhpPanel.tsx          # PHP 版本管理
│       ├── InstallLnmp.tsx       # LNMP 环境安装器
│       ├── MonitorPanel.tsx      # 系统监控
│       ├── FirewallPanel.tsx     # 防火墙规则
│       ├── SslPanel.tsx          # SSL 证书管理
│       ├── BbrPanel.tsx          # BBR 拥塞控制
│       ├── SoftwareRepo.tsx      # 软件包管理
│       ├── SiteLogsPanel.tsx     # 站点日志查看器
│       └── ServerSettingsPanel.tsx # 服务器设置
```

## 开发

### 环境要求
- [Node.js](https://nodejs.org/)（v18+）
- [Rust](https://www.rust-lang.org/tools/install)
- [Tauri](https://v2.tauri.app/start/prerequisites/) 所需的系统依赖

### 启动

```bash
# 安装依赖
npm install

# 启动开发服务
npm run tauri dev

# 生产环境构建
npm run tauri build
```

## 许可证

私有 — 保留所有权利。
# LeePanel

基于 Tauri 2 和 React 构建的跨平台 SSH 服务器管理桌面应用。LeePanel 为运维工程师和开发者提供一站式远程管理体验。

[English](README.md)

## 功能特性

### 连接与终端
- SSH 密码 / 密钥认证
- 基于 xterm.js 的全功能终端，支持剪贴板和超链接
- 断线自动重连

### 文件管理
- 远程文件浏览器，支持拖拽上传/下载
- 压缩包管理（zip、tar.gz）
- 文件权限管理
- 收藏夹快速访问

### LNMP 环境管理
- 一键安装 Nginx、MySQL/MariaDB、PHP-FPM
- 服务启动 / 停止 / 重启 / 重载控制
- 实时状态监控

### 站点管理
- Nginx 虚拟主机创建与配置
- Let's Encrypt SSL 证书管理
- 反向代理配置（支持 WebSocket）
- 防盗链保护
- 站点级 PHP 版本切换
- Rewrite 规则管理

### 数据库管理
- MySQL/MariaDB 数据库增删改查
- 用户权限管理（localhost / 任意主机 / 指定 IP）
- 数据库备份与恢复（zip 格式）
- SQL 文件导入
- root 密码管理

### Redis 管理
- 基于 SCAN 的 Key 浏览与分页
- Key 增删改查，支持 TTL
- 多数据库（DB0–DB15）切换
- 数据库清空确认
- 备份与恢复

### 系统与网络
- CPU、内存、磁盘、网络实时监控
- 进程列表与资源占用
- 防火墙规则管理（ufw / firewalld / iptables）
- BBR TCP 拥塞控制
- 系统信息仪表盘

### 服务器设置
- 自动重连配置
- 文件缓存管理
- 软件更新检查
- 服务器重启（正常 / 强制）
- SSH 认证模式（密码 / 密钥）管理
- SSH 密钥生成与部署

### 多语言支持
- 英文与简体中文
- 语言偏好本地持久化
- 应用内语言切换器

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri 2.x |
| 前端 | React 19 + TypeScript |
| 构建工具 | Vite 8 |
| 终端 | xterm.js 6 |
| SSH 客户端 | russh (Rust) |
| 存储 | SQLite (rusqlite) |
| 国际化 | react-i18next |

## 开发

### 环境要求
- [Node.js](https://nodejs.org/)（v18+）
- [Rust](https://www.rust-lang.org/tools/install)
- [Tauri](https://v2.tauri.app/start/prerequisites/) 所需的系统依赖

### 启动

```bash
# 安装依赖
npm install

# 启动开发服务
npm run tauri dev

# 生产环境构建
npm run tauri build
```

## 项目结构

```
src/
├── App.tsx                  # 根组件
├── main.tsx                 # 入口文件
├── i18n/
│   ├── index.ts             # i18next 初始化
│   ├── en.json              # 英文翻译
│   └── zh-CN.json           # 中文翻译
├── components/
│   ├── Sidebar.tsx          # 服务器列表 + 语言切换器
│   ├── ServerPanel.tsx      # 导航栏
│   ├── FileBrowser.tsx      # 远程文件管理器
│   ├── Terminal.tsx         # xterm.js 终端
│   └── panels/
│       ├── Dashboard.tsx    # 系统概览
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
├── src/                     # Rust 后端（SSH、SQLite、系统命令）
└── tauri.conf.json          # Tauri 配置
```

## 许可证

私有 — 保留所有权利。
