# 多服务器同时连接 — 技术分析

好消息：**后端已经天然支持多会话**。瓶颈完全在前端 UI 层。

---

## 现状分析

| 层 | 现状 | 多会话就绪？ |
|---|---|---|
| Rust 后端 `SshManager` (ssh.rs#L107-L111) | `HashMap<String, SshSession>` 按 session_id 存储，互不干扰 | **已就绪** |
| 事件系统 | `ssh-output` / `ssh-disconnected` 等事件均携带 `sessionId` | **已就绪** |
| 前端 `App.tsx` (#L55-L56) | 单一 `sessionId` 状态，连接新服务器前**强制断开**旧连接 | **瓶颈** |
| 前端 `ServerPanel` | 只渲染一个实例，Terminal/FileBrowser 绑定单一 session | **瓶颈** |

关键限制代码在 `handleDirectConnect` (App.tsx#L604-L610)：

```ts
// If already connected, disconnect first
if (sessionId) {
  manualDisconnectRef.current = true
  await invoke('ssh_disconnect', { sessionId }).catch(() => {})
  clearSession()
}
```

---

## 实现方案：Tab 式多会话

**核心改动思路**：

```
单 sessionId  →  sessions: Map<configId, { sessionId, host, username, ... }>
单 ServerPanel →  多个 ServerPanel 实例（display:none 切换）
```

### 1. 前端状态改造（App.tsx）

```ts
interface ActiveSession {
  configId: string
  sessionId: string
  name: string
  host: string
  username: string
}

// 替换原来的单一 sessionId
const [sessions, setSessions] = useState<ActiveSession[]>([])
const [activeTab, setActiveTab] = useState<string | null>(null) // configId
```

- 点击"连接"→ 新增一个 session 到数组，不 disconnect 旧的
- 点击侧边栏已连接服务器 → 切换 `activeTab`
- 每个 session 渲染独立的 `<ServerPanel key={s.configId} />`

### 2. ServerPanel 多实例

```tsx
{sessions.map(s => (
  <div key={s.configId} style={{ display: s.configId === activeTab ? 'block' : 'none' }}>
    <ServerPanel sessionId={s.sessionId} connHost={s.host} ... />
  </div>
))}
```

用 `display:none` 而非卸载，保持 Terminal 和 FileBrowser 状态不丢失（当前已有这个模式）。

### 3. 事件路由（无需改动）

后端事件已包含 `sessionId`，每个 Terminal 组件内部通过 `listen('ssh-output')` + 过滤 `sessionId` 接收数据，多实例互不干扰。

### 4. 断开连接

- 单个断开：关闭 tab 时 `invoke('ssh_disconnect', { sessionId })`
- 全部断开：遍历 sessions 逐个 disconnect

---

## 需要关注的问题

| 问题 | 影响 | 对策 |
|---|---|---|
| **内存** | 每个 xterm.js 实例 ~5-15MB，每个 SSH 连接 ~2-5MB | 限制最大同时连接数（如 5-8 个） |
| **Terminal 数量** | 隐藏的 xterm 仍占 DOM | 可考虑超过 N 个时销毁最旧的 |
| **自动重连** | 需要 per-session 独立重连逻辑 | 重连状态绑定到具体 configId |
| **上传队列** | 当前上传绑定单一 sessionId | 上传面板需关联到具体 session |
| **侧边栏状态** | 需显示多个"已连接"标记 | `currentSessionId` 改为 `connectedIds: string[]` |
| **Tab 栏 UI** | 需要新增 tab 切换条 | 放在 main-area 顶部 |

---

## 工作量估算

| 模块 | 改动量 |
|---|---|
| `App.tsx` 状态管理重构 | 中等（~150 行改动） |
| Tab 栏 UI 组件 | 新增（~80 行） |
| `Sidebar.tsx` 多连接状态 | 小（~30 行） |
| `ServerPanel` 适配 | 极小（props 不变，只是多实例） |
| 后端 Rust | **零改动** |
| 自动重连逻辑适配 | 中等（per-session 重连） |
| CSS | 小量 |

---

## 总结

后端零改动，核心工作在前端 `App.tsx` 的状态从"单会话"升级为"会话列表" + 一个 Tab 切换 UI。架构上完全可行，且改动面可控。
