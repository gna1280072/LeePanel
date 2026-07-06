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
