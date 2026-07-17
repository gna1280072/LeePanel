# Tauri v2 E2E 测试方案分析

## 核心难点

当前项目是 Tauri v2 桌面应用，E2E 测试面临三个层面的挑战：

1. **WebView2 渲染层**：UI 跑在 WebView2 里，不是普通浏览器
2. **Rust IPC 层**：前端通过 `invoke()` 调 Rust 命令，E2E 必须覆盖这条链路
3. **SSH 远程层**：几乎所有功能都要 SSH 到真实服务器

## 方案一：Tauri Driver（官方方案）

Tauri 官方提供的 WebDriver 方案，基于 W3C WebDriver 协议。

**原理**：启动 Tauri 应用 → WebDriver 连接 WebView2 → 用 Selenium/Playwright 语法操作 DOM

**依赖**：
- `tauri-driver` CLI（类似 chromedriver）
- Windows 上需要 WebView2 Runtime + MS Edge WebDriver

**可行性问题**：
- Tauri v2 的 `tauri-driver` 目前仍在**实验阶段**，文档稀少
- Windows 上配置 WebView2 WebDriver 比 macOS/Linux 复杂得多
- 每次测试启动完整桌面应用，**速度慢**（5-10 秒/次）
- 需要真实 SSH 服务器或 mock 服务器配合

**评价**：理论最完整，实操坑最多，投入产出比低。

## 方案二：Playwright + Tauri（推荐探索方向）

用 Playwright 连接 WebView2 的 Chromium 内核。

**原理**：WebView2 底层是 Chromium，可以通过 `--remote-debugging-port` 暴露 DevTools Protocol，Playwright 直接连上去。

**关键步骤**：
1. 启动 Tauri 应用时注入 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`
2. Playwright `connectOverCDP('http://localhost:9222')` 连接
3. 用标准 Playwright API 操作 DOM、断言 UI

**优势**：
- Playwright 生态成熟，API 好用
- 可以直接截图对比
- 不需要 `tauri-driver`

**劣势**：
- WebView2 的 CDP 端口暴露**不稳定**，取决于 WebView2 版本
- 需要在 CI 环境（GitHub Actions Windows runner）上跑 WebView2，配置复杂

## 方案三：Mock 驱动的集成 E2E（最务实）

**思路**：不启动真实 Tauri 应用，而是在浏览器里跑前端，用 mock server 替代 Rust 后端。

**做法**：
1. `vite dev` 启动前端（普通浏览器）
2. 写一个 `invoke()` 拦截器，根据命令名返回预设 JSON
3. 用 Playwright 操作浏览器中的前端 UI
4. 验证 UI 状态变化、页面跳转、i18n 渲染

**可覆盖的场景**：
- 连接表单提交 → 验证 UI 状态切换
- 文件浏览器：mock `ssh_list_dir` 返回 → 验证文件列表渲染
- 数据库面板：mock `server_list_databases` → 验证表格渲染
- 右键菜单：验证菜单项文案（如刚改的"保存到本地电脑"）
- 多语言切换：遍历 10 种语言，截图对比

**不可覆盖的场景**：
- 真实 SSH 连接/断开
- Rust 端的 SQLite 操作
- 文件上传/下载的实际传输

**评价**：覆盖 70%+ 的 UI 回归，速度快（秒级），CI 友好。

## 方案四：SSH 服务器容器化（补充方案三的后端）

**思路**：用 Docker 启动一个 SSH 服务器容器，让真实 Tauri 应用连上去。

```
docker run -d -p 2222:22 linuxserver/openssh-server
```

然后在 E2E 测试中：
- 添加一个连接配置指向 `localhost:2222`
- 执行真实的文件操作、命令执行
- 验证结果

**限制**：
- 只能测试基础 SSH 操作（ls、cat、mkdir 等）
- 无法测试需要 root 权限的操作（安装软件、修改系统配置）
- CI 环境需要 Docker 支持

## 推荐路线

| 阶段 | 方案 | 覆盖范围 | 投入 |
|---|---|---|---|
| **现在** | 方案三（Mock E2E） | UI 渲染 + 交互回归 | 小 |
| **下一步** | 方案二（Playwright + WebView2 CDP） | 真实 Tauri 渲染 | 中 |
| **远期** | 方案一（Tauri Driver） | 官方标准 E2E | 大（等官方成熟） |

**最务实的起步点**是方案三——用 Playwright 跑 `vite dev` 的前端，mock 掉 `invoke()`。这样：
- 不需要 Tauri 运行时
- 不需要 SSH 服务器
- GitHub Actions Linux runner 就能跑（不需要 Windows）
- 每次测试 1-2 秒
