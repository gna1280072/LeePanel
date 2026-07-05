# LNMP 管理面板技术方案

## 核心架构思路

当前应用是**桌面 SSH 客户端**（Tauri + React + Rust），所有服务器操作通过 SSH 远程执行命令完成。这与宝塔面板（在服务器端运行的 Web 应用）有本质区别：

| 对比 | 宝塔面板 | 本方案 |
|------|---------|--------|
| 运行位置 | 服务器端 | 本地桌面 |
| 通信方式 | HTTP API | SSH 命令 |
| 服务端 agent | 需要安装 | 不需要 |
| 实时输出 | WebSocket | 已有 xterm.js 终端 |

**核心策略：所有操作通过 SSH exec channel 执行远程命令，复杂操作（如安装）生成 shell 脚本后通过终端执行，用户可以在终端中看到实时进度。**

## UI 布局重构

当前布局是上下分栏（文件浏览器 + 终端），需要改为 Tab 切换模式：

```
+----------+-------------------------------------+
| Sidebar  | Menu Bar                            |
|          +-------------------------------------+
|          | [Files] [Terminal] [Server Panel]   |
|          +-------------------------------------+
|          |                                     |
|          |  Active Panel Content               |
|          |                                     |
|          |  - Files: 文件浏览器 (现有)           |
|          |  - Terminal: 终端 (现有)             |
|          |  - Server Panel: LNMP管理 (新增)    |
|          |                                     |
+----------+-------------------------------------+
```

终端保持底部可折叠，任何 Tab 下都能看到终端输出（安装脚本自动切换到终端 Tab 执行）。

## 分阶段开发计划

---

### 阶段 1: 基础架构 + 系统信息面板

**目标**：搭建 Panel 框架和远程命令执行基础能力

**新增文件**：
- `src/components/ServerPanel.tsx` — 服务器管理主面板
- `src/components/panels/SystemInfo.tsx` — 系统信息（OS、内核、CPU、内存、磁盘、负载）
- `src/components/panels/Dashboard.tsx` — 仪表盘概览
- `src-tauri/src/server.rs` — 服务器管理相关 Rust 后端

**Rust 后端新增**：

在 `ssh.rs` 中已有 `exec` channel 能力，新增一个通用方法：

```rust
// ssh.rs - 新增通用命令执行方法（带输出收集）
pub async fn exec_with_output(
    &self, session_id: &str, cmd: &str, timeout_secs: u64
) -> Result<(String, String, i32), String>
// 返回 (stdout, stderr, exit_code)
```

新增 `server.rs` 模块，封装高层操作：
```rust
pub struct ServerManager;

impl ServerManager {
    // 检测操作系统类型和版本
    pub async fn detect_os(ssh_mgr: &SshManager, sid: &str) -> Result<OsInfo, String>
    // 获取系统资源信息
    pub async fn get_system_info(ssh_mgr: &SshManager, sid: &str) -> Result<SystemInfo, String>
}
```

OS 检测命令（兼容两大系列）：
```bash
# 检测发行版
cat /etc/os-release 2>/dev/null || cat /etc/redhat-release 2>/dev/null
# 系统信息一次性获取
uname -a; cat /proc/cpuinfo | grep 'model name' | head -1;
free -m; df -h; uptime; cat /proc/loadavg
```

**前端面板结构**：
```
ServerPanel
├── 顶部状态栏（OS图标 + 主机名 + 在线时长）
├── 左侧导航
│   ├── Dashboard（概览）
│   ├── Nginx
│   ├── MySQL
│   ├── PHP
│   ├── Sites（站点管理）
│   └── Monitor（监控）
└── 右侧内容区
```

Dashboard 显示：
- 系统：发行版 + 内核版本 + 架构
- 负载：CPU使用率 + 内存 + Swap + 磁盘
- 服务状态：Nginx / MySQL / PHP-FPM 运行状态（绿灯/红灯）
- 快捷操作：一键安装 LNMP 按钮

---

### 阶段 2: LNMP 一键安装

**目标**：检测系统并一键安装 Nginx + MySQL + PHP

**安装策略（shell 脚本）**：

根据检测到的 OS 类型生成不同脚本：

**CentOS/RHEL 系（yum/dnf）**：
```bash
#!/bin/bash
set -e
echo "=== Installing LNMP on CentOS/RHEL ==="
# 添加 EPEL 和 Remi 源
yum install -y epel-release
yum install -y https://rpms.remirepo.net/enterprise/remi-release-$(rpm -E %rhel).rpm
# 安装 Nginx
yum install -y nginx
systemctl enable --now nginx
# 安装 MySQL 8.0
yum install -y mysql-server
systemctl enable --now mysqld
# 安装 PHP 8.x
yum module enable -y php:remi-8.2
yum install -y php-fpm php-mysqlnd php-mbstring php-xml php-gd
systemctl enable --now php-fpm
echo "=== LNMP Install Complete ==="
```

**Debian/Ubuntu 系（apt）**：
```bash
#!/bin/bash
set -e
echo "=== Installing LNMP on Debian/Ubuntu ==="
apt update
# 安装 Nginx
apt install -y nginx
systemctl enable --now nginx
# 安装 MySQL
apt install -y mysql-server
systemctl enable --now mysql
# 安装 PHP
apt install -y php-fpm php-mysql php-mbstring php-xml php-gd
systemctl enable --now php-fpm
echo "=== LNMP Install Complete ==="
```

**实现方式**：
1. 前端点击"一键安装 LNMP"
2. Rust 后端根据 OS 类型生成脚本内容
3. 通过 SSH exec 将脚本写入 `/tmp/lnmp-install.sh` 并执行
4. 自动切换到终端 Tab，实时显示安装进度
5. 安装完成后检测各服务状态，更新 Dashboard

**新增 Rust 代码**：
```rust
// server.rs
pub async fn install_lnmp(
    ssh_mgr: &SshManager, sid: &str, os_type: &str,
    components: &LnmpConfig, app: &AppHandle
) -> Result<(), String>
// 生成脚本 → 上传到 /tmp → 通过终端 channel 执行（带实时输出）
```

**前端安装界面**：
- 组件选择：Nginx / MySQL(5.7|8.0) / PHP(7.4|8.1|8.2|8.3)
- 安装进度条 + 终端实时日志
- 安装完成后自动检测并显示状态

---

### 阶段 3: 服务管理

**目标**：Nginx / MySQL / PHP-FPM 的启动、停止、重启、重载、状态查看

**通用服务操作命令**（两大系统通用，systemd）：
```bash
systemctl start nginx
systemctl stop nginx
systemctl restart nginx
systemctl reload nginx        # Nginx 特有
systemctl status nginx
systemctl is-active nginx     # 返回 active/inactive
journalctl -u nginx --no-pager -n 50  # 查看最近日志
```

MySQL 额外操作：
```bash
mysqladmin -u root -p status       # MySQL 状态
mysql -u root -p -e "SHOW VARIABLES LIKE 'version';"  # 版本
```

**新增 Rust**：
```rust
pub async fn service_action(
    ssh_mgr: &SshManager, sid: &str,
    service: &str, action: &str  // start|stop|restart|reload|status
) -> Result<ServiceResult, String>
```

**前端服务管理界面**：
```
+-- Nginx -----------------------------------+
|  Status: [Running]  Version: 1.24.0        |
|  [Start] [Stop] [Restart] [Reload]         |
|  [View Logs]  [Edit Config]                |
+--------------------------------------------+
```

---

### 阶段 4: 配置文件编辑 + 站点管理

**目标**：可视化编辑 Nginx/MySQL/PHP 配置文件，管理虚拟主机

**配置文件路径**（自动检测）：

| 组件 | CentOS/RHEL | Debian/Ubuntu |
|------|-------------|---------------|
| Nginx主配置 | /etc/nginx/nginx.conf | /etc/nginx/nginx.conf |
| Nginx站点 | /etc/nginx/conf.d/ | /etc/nginx/sites-available/ |
| MySQL | /etc/my.cnf | /etc/mysql/mysql.conf.d/mysqld.cnf |
| PHP | /etc/php.ini | /etc/php/8.x/fpm/php.ini |
| PHP-FPM | /etc/php-fpm.d/www.conf | /etc/php/8.x/fpm/pool.d/www.conf |

**实现**：复用已有的 SFTP 读写能力（`ssh_read_file` / `ssh_write_file`），前端用代码编辑器（如 Monaco Editor 或 CodeMirror）展示配置文件，支持语法高亮。

**站点管理**：
- 列出所有虚拟主机（解析 nginx conf.d 或 sites-available）
- 新增站点向导（域名、根目录、PHP版本、SSL）
- 自动配置 Let's Encrypt SSL（通过 certbot）

```rust
pub async fn list_sites(ssh_mgr: &SshManager, sid: &str) -> Result<Vec<Site>, String>
pub async fn create_site(ssh_mgr: &SshManager, sid: &str, config: &SiteConfig) -> Result<(), String>
pub async fn setup_ssl(ssh_mgr: &SshManager, sid: &str, domain: &str) -> Result<(), String>
```

---

### 阶段 5: 系统监控 + 防火墙 + 定时任务

**系统监控**：
```bash
# 实时指标（每2秒刷新）
top -bn1 | head -20
iostat -x 1 2
df -h
free -m
```

前端用图表展示（可用 recharts 或 chart.js）：CPU、内存、磁盘IO 历史曲线。

**防火墙管理**：
```bash
# CentOS: firewalld
firewall-cmd --list-all
firewall-cmd --add-port=80/tcp --permanent
# Ubuntu: ufw
ufw status
ufw allow 80/tcp
```

**定时任务（Cron）**：
```bash
crontab -l          # 列出
crontab -e          # 编辑（通过写入临时文件实现）
```

---

## 文件结构规划

```
src-tauri/src/
├── ssh.rs          # SSH 连接管理（现有，新增 exec_with_output）
├── server.rs       # 新增：服务器管理高层逻辑
│   ├── detect_os()
│   ├── get_system_info()
│   ├── install_lnmp()
│   ├── service_action()
│   ├── list_sites() / create_site()
│   └── ...
├── lib.rs          # 新增 Tauri commands 注册
└── config.rs       # 现有

src/components/
├── ServerPanel.tsx       # 新增：面板主容器（Tab + 侧栏导航）
├── panels/
│   ├── Dashboard.tsx     # 新增：概览仪表盘
│   ├── ServiceCard.tsx   # 新增：服务管理卡片
│   ├── SiteManager.tsx   # 新增：站点管理
│   ├── ConfigEditor.tsx  # 新增：配置编辑器
│   ├── Monitor.tsx       # 新增：系统监控图表
│   └── Firewall.tsx      # 新增：防火墙规则
├── FileBrowser.tsx       # 现有
├── Terminal.tsx          # 现有（改为可折叠底部面板）
├── Sidebar.tsx           # 现有
└── ConnectForm.tsx       # 现有

src/App.tsx               # 改造：添加 Tab 切换逻辑
src/App.css               # 新增面板样式
```

## 建议开发顺序

| 阶段 | 内容 | 预计工作量 | 优先级 |
|------|------|-----------|--------|
| 1 | Tab框架 + 系统信息面板 | 中 | 最高 |
| 2 | 一键安装 LNMP | 大 | 高 |
| 3 | 服务管理（启停重启） | 中 | 高 |
| 4 | 配置编辑 + 站点管理 | 大 | 中 |
| 5 | 监控图表 + 防火墙 + Cron | 大 | 低 |

建议从阶段 1 开始逐步实现，每个阶段独立可用。是否确认这个方案，从阶段 1 开始实施？
