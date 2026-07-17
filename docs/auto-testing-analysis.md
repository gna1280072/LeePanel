# 自动测试可行性分析

## 项目现状

| 层 | 技术栈 | 现有测试 |
|---|---|---|
| 前端 | React 19 + Vite + i18next | 无 |
| 后端 | Rust (Tauri v2) | 无 |
| 核心依赖 | SSH 远程服务器连接 | — |

核心难点：**几乎所有功能都要经过 SSH 到远程服务器**，手工测试麻烦正是因为要连真实服务器才能验证。

---

## 分层测试策略

### 1. Rust 后端单元测试（`cargo test`，最容易落地）

**可测的部分（纯逻辑，无需 SSH）：**
- config.rs — Connection/Settings 的序列化、CRUD
- db.rs — SQLite 初始化、FbFavorites、FbDirCache、SiteMetadata 等纯数据库操作
- server.rs 中的 Shell 命令拼接函数（如 escape 逻辑、Nginx 配置生成）
- `SshCache` 的 TTL 失效、`invalidate()` 行为

**收益最高**：db.rs 和 cache 逻辑出 bug 影响全局，且完全可以用内存 SQLite (`:memory:`) 隔离测试，零外部依赖。

### 2. Rust 集成测试（Mock SSH Server）

可以用 mock-ssh-server 或类似的 crate 在本地启动一个假 SSH 服务，测试：
- `session_exec_with_output` 的返回值解析
- `session_list_dir` 的 SFTP 响应处理
- 各种 `server::` 函数对 SSH 输出的解析逻辑

**工作量中等**，但能覆盖 server.rs 那 8952 行中大量的字符串解析逻辑。

### 3. 前端单元测试（Vitest）

```
npm install -D vitest @testing-library/react jsdom
```

- **i18n 完整性测试**：检查所有 10 个语言包的 key 是否和 en.json 一致（防止漏翻译）
- **组件渲染测试**：mock `invoke()` 返回值，验证 Sidebar、Dashboard、DatabasePanel 等组件的 UI 状态
- **App.tsx 状态机测试**：mock Tauri API，测试连接/断开/重连等状态流转

**关键**：前端测试时 `invoke()` 必须 mock，因为不在 Tauri 环境运行。

### 4. E2E 测试（最重，ROI 最低）

- Tauri 官方有 `tauri-driver`（基于 WebDriver），但配置复杂
- 需要真实或 Docker 化的 Linux 服务器做 SSH target
- **不推荐现阶段投入**

---

## 建议优先级

| 优先级 | 层 | 预估工作量 | 覆盖范围 |
|---|---|---|---|
| **P0** | Rust 单元测试（db + cache + config） | 小 | 核心数据层 |
| **P1** | 前端 i18n key 一致性测试 | 极小 | 防止漏翻译 |
| **P2** | 前端组件测试（mock invoke） | 中 | UI 回归 |
| **P3** | Rust Mock SSH 集成测试 | 中大 | server.rs 解析逻辑 |
| **P4** | E2E（Tauri Driver） | 大 | 端到端 |

---

## 关键判断

P0+P1 是**投入产出比最高**的起点——db.rs/config.rs 的单元测试可以在一个下午内建立，i18n 一致性测试只需几十行代码，两者都能在 CI（GitHub Actions）里自动跑。
