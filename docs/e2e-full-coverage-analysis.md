# Tauri v2 E2E 100% 覆盖方案分析

## 全链路架构

要做到 100% 覆盖，核心问题是：**所有功能链路都是 前端 UI → Rust invoke → SSH 远程执行**，所以 E2E 必须同时覆盖三层。

```
Playwright (浏览器)
    │  操作 DOM / 断言 UI
    ▼
Vite Dev Server (前端 React)
    │  window.__TAURI__.invoke(cmd, args)
    ▼
Mock Invoke Proxy (Express/Fastify)
    │  根据 cmd 路由到对应 handler
    ▼
Docker SSH 容器 (真实 Linux)
    │  执行真实 shell 命令
    ▼
验证结果 (UI 断言 + 容器内状态检查)
```

## 需要的基础设施

### 1. SSH 容器集群（模拟真实服务器）

单个容器不够，需要多个角色：

| 容器 | 用途 | 镜像 |
|---|---|---|
| **ubuntu-server** | 基础功能测试（文件、系统信息、服务） | `ubuntu:22.04` + openssh |
| **debian-server** | Debian 系包管理测试（apt） | `debian:12` + openssh |
| **centos-server** | RHEL 系包管理测试（dnf/yum） | `rockylinux:9` + openssh |
| **mysql-server** | 数据库功能测试 | `mysql:8.0` 官方镜像 |
| **mariadb-server** | MariaDB 功能测试 | `mariadb:11` 官方镜像 |

用 `docker-compose.yml` 统一管理，一次性 `docker compose up`。

### 2. Mock Invoke Proxy

在 `vite dev` 和前端之间插入一层 HTTP 代理，拦截 `invoke()` 调用：

```
前端 invoke('ssh_connect', {host, port, user, pass})
  → HTTP POST /api/ssh_connect
  → Proxy 真正 SSH 到容器执行命令
  → 返回结果给前端
```

**关键点**：不是返回假数据，而是**真的 SSH 到 Docker 容器执行命令**，然后把结果返回给前端。这样前端拿到的数据和真实 Tauri 运行时完全一致。

### 3. 容器预置状态

每个容器需要预装：
- openssh-server + 配置好密码/密钥登录
- 基础工具（curl, wget, tar, zip, unzip）
- nginx/apache（用于站点管理测试）
- PHP（用于 PHP 版本管理测试）
- 测试用的文件和目录（用于文件浏览器测试）

## 功能覆盖矩阵

| 功能模块 | E2E 可测性 | 需要的容器/条件 | 难度 |
|---|---|---|---|
| **SSH 连接/断开** | ✅ 完全可测 | 任意 SSH 容器 | ★☆☆ |
| **Dashboard 系统信息** | ✅ 完全可测 | SSH 容器 | ★☆☆ |
| **文件浏览器** | ✅ 完全可测 | SSH 容器 + 预置文件 | ★★☆ |
| **文件上传/下载** | ✅ 完全可测 | SSH 容器 + SFTP | ★★☆ |
| **终端 (xterm)** | ✅ 完全可测 | SSH 容器 | ★★☆ |
| **站点管理** | ✅ 完全可测 | nginx 容器 | ★★★ |
| **SSL 证书** | ✅ 完全可测 | nginx + certbot（用自签证书） | ★★★ |
| **数据库管理** | ✅ 完全可测 | mysql/mariadb 容器 | ★★★ |
| **数据库备份/恢复** | ✅ 完全可测 | mysql 容器 + 预置数据 | ★★★ |
| **防火墙管理** | ⚠️ 部分可测 | 需要 privileged 容器 | ★★★ |
| **BBR 优化** | ⚠️ 部分可测 | 需要内核参数修改权限 | ★★★★ |
| **Docker 管理** | ❌ 难以测试 | Docker-in-Docker，极不稳定 | ★★★★★ |
| **软件安装 (LNMP)** | ⚠️ 部分可测 | 安装耗时 5-30 分钟，CI 太慢 | ★★★★ |
| **PHP 源码编译** | ⚠️ 部分可测 | 编译耗时 10-20 分钟 | ★★★★ |
| **监控面板** | ✅ 可测 | SSH 容器返回模拟数据 | ★★☆ |
| **服务器设置** | ✅ 可测 | SSH 容器 | ★★☆ |
| **i18n 多语言** | ✅ 完全可测 | 不需要 SSH，纯前端 | ★☆☆ |
| **收藏夹/备注** | ✅ 完全可测 | Mock proxy 的本地 SQLite | ★☆☆ |
| **自定义软件** | ✅ 可测 | SSH 容器 | ★★☆ |

## 不可达的 5% 及应对

| 功能 | 为什么难 | 替代方案 |
|---|---|---|
| **Docker-in-Docker** | 容器里跑 Docker 需要 `--privileged`，CI 上极不稳定 | 单元/集成测试覆盖 Rust 端的 Docker 命令生成逻辑，E2E 只验证 UI 渲染 |
| **内核参数 (BBR)** | 修改 `sysctl` 需要特权容器 | 验证 UI 读取/展示正确，不验证实际内核变更 |
| **长时间安装** | apt 装 MySQL 要 5 分钟，PHP 编译要 20 分钟 | 拆成"触发安装 → 验证命令发出 → 模拟安装完成 → 验证 UI 更新" |
| **防火墙 iptables** | 修改防火墙规则可能导致容器断网 | 只验证命令生成，不验证实际规则生效 |
| **真实域名 SSL** | Let's Encrypt 需要真实域名 + DNS | 用自签证书 + hosts 文件模拟 |

## CI 集成方案（GitHub Actions）

```yaml
# 大致流程
jobs:
  e2e:
    runs-on: ubuntu-latest  # 不需要 Windows runner
    steps:
      - uses: actions/checkout@v4
      - run: docker compose up -d          # 启动 SSH 容器集群
      - run: npm ci
      - run: npx playwright install chromium
      - run: node mock-proxy/server.js &   # 启动 Mock Invoke Proxy
      - run: npx vite dev --port 5173 &    # 启动前端
      - run: npx playwright test           # 跑 E2E
      - run: docker compose logs           # 失败时打印容器日志
```

**耗时预估**：
- 容器启动：~30 秒
- 前端 + proxy 启动：~10 秒
- 测试执行：~2-3 分钟（不含长安装）
- 总计：**3-4 分钟/次**

## 实际覆盖率评估

| 类别 | 覆盖率 | 说明 |
|---|---|---|
| UI 渲染 + 交互 | **100%** | Playwright 全覆盖 |
| SSH 命令链路 | **90%** | 除 Docker/BBR 外全覆盖 |
| 数据 CRUD | **95%** | 收藏夹/备注/凭据/站点元数据全覆盖 |
| 文件操作 | **95%** | 上传/下载/压缩/解压/编辑全覆盖 |
| 数据库管理 | **90%** | CRUD/备份/恢复/权限全覆盖 |
| 软件安装 | **60%** | 触发+验证覆盖，完整安装流程太慢 |
| 系统级操作 | **40%** | Docker/BBR/防火墙受限 |
| **综合** | **~85-90%** | 实际可达的最高覆盖率 |

## 结论

**真实 100% 覆盖不可能**——Docker-in-Docker 和内核操作在 CI 容器里就是跑不起来，这是物理限制，不是工程问题。

**最优策略**：
1. **E2E（~85%）**：Mock Invoke Proxy + Docker SSH 容器集群，覆盖所有 UI + SSH 链路
2. **Rust 单元测试（已做）**：覆盖 SQLite CRUD、缓存、解析逻辑
3. **前端组件测试**：覆盖 E2E 难以触发的边界状态（空数据、错误状态、loading 态）
4. **剩余 5%**：手工测试 + 代码审查，接受这个 gap
