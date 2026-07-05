# LeePanel

基于 Tauri 2 的 SSH 服务器Linux管理工具，集成终端、文件管理、LNMP 环境管理、站点管理等功能。所有配置数据使用 SQLite 本地持久化存储。

## 功能

**连接与终端**
- SSH 连接管理（密码 / 密钥认证）
- 集成 xterm.js 终端，支持剪贴板、Web 链接
- 断线自动重连（可配置间隔和次数）

**文件管理**
- 远程文件浏览（目录导航、排序、搜索）
- 文件上传 / 下载（支持拖拽、断点续传）
- 压缩 / 解压、权限设置
- 收藏夹（按服务器独立存储）

**LNMP 环境**
- 一键安装 LNMP（Nginx + MySQL + PHP）
- 服务管理（启动 / 停止 / 重启 / 状态查看）
- MySQL 进程查看与 SQL 查询

**站点管理**
- 创建 / 编辑 / 删除 Nginx 虚拟主机
- SSL 证书自动申请（Let's Encrypt / certbot）
- 反向代理配置
- 防盗链设置
- PHP 多版本支持、运行目录、open_basedir
- 站点日志查看

**系统与网络**
- 系统信息仪表盘（CPU、内存、磁盘、负载）
- 防火墙规则管理
- BBR 网络加速开关
- Docker 容器与镜像管理
- 软件仓库

**数据存储**
- 全部配置持久化到 SQLite（连接信息、站点设置、收藏夹、UI 状态）
- 按服务器 IP 区分站点数据

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | React 19 + TypeScript + Vite |
| 桌面框架 | Tauri 2.x |
| 后端 | Rust（russh SSH 客户端 + tokio 异步运行时） |
| 终端 | xterm.js 6.x |
| 存储 | SQLite（rusqlite） |
| 构建 | GitHub Actions（Windows MSI/NSIS） |

## 开发

```bash
# 环境要求：Node.js 20+、Rust stable、Git

git clone https://github.com/gna1280072/ssh-tool.git
cd ssh-tool
npm install

# 开发模式（Vite dev server + Rust 窗口）
npx tauri dev

# 生产构建（输出到 src-tauri/target/release/bundle/）
npx tauri build
```

## 发布

通过 GitHub Actions 自动构建，推送 tag 触发：

```bash
# 更新 src-tauri/tauri.conf.json 中的版本号，然后：
git tag v1.0.0
git push origin v1.0.0
```

也可在 Actions 页面手动触发构建。

## 项目结构

```
src/                        # React 前端
├── components/
│   ├── panels/             # 功能面板（Dashboard、Sites、Docker、Firewall...）
│   ├── Sidebar.tsx         # 连接管理侧边栏
│   ├── Terminal.tsx        # 终端组件
│   ├── FileBrowser.tsx     # 文件浏览器
│   └── ServerPanel.tsx     # 服务器面板主入口
├── App.tsx                 # 应用主组件
└── App.css                 # 全局样式

src-tauri/src/              # Rust 后端
├── lib.rs                  # Tauri 命令注册
├── ssh.rs                  # SSH 会话管理（russh）
├── server.rs               # 服务器操作（Nginx、MySQL、PHP、Docker...）
├── db.rs                   # SQLite 数据库初始化与数据管理
├── config.rs               # 连接、收藏、设置的数据模型与 CRUD
└── main.rs                 # 入口
```

## License

MIT
