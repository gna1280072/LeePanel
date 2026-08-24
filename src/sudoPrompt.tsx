import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke as rawInvoke } from '@tauri-apps/api/core'

// 权限模型 v8：sudo 密码交互（ask 模式）
// - 后端 exec 层在需要 sudo 但会话无密码时返回约定错误码
// - 全局 invoke 包装：任何命令返回 SUDO_PASSWORD_REQUIRED/INCORRECT 时
//   自动弹出输入框 → 设置会话级 sudo 密码 → 重试一次
// - 各面板无需逐个接 invokeWithSudo；import 统一改为本模块的 invoke 即可

export const SUDO_REQUIRED = 'SUDO_PASSWORD_REQUIRED'
export const SUDO_INCORRECT = 'SUDO_PASSWORD_INCORRECT'

// ===== 弹窗请求队列 =====
// FIFO + 同 session 并发合并：同一会话的多个并发请求共享一次输入结果，
// 不同会话排队依次弹窗（避免多窗叠放 / 挂起的 Promise 永不 resolve）。

interface SudoRequest {
  sessionId: string
  incorrect: boolean
  resolve: (pw: string | null) => void
}

let queue: SudoRequest[] = []
let active: SudoRequest | null = null
let fanout: ((pw: string | null) => void)[] = []
const listeners = new Set<(r: SudoRequest | null) => void>()

function notify(r: SudoRequest | null) {
  listeners.forEach((l) => l(r))
}

function pump() {
  if (active || queue.length === 0) return
  active = queue.shift()!
  fanout = []
  notify(active)
}

function settle(pw: string | null) {
  if (!active) return
  const resolves = [active.resolve, ...fanout]
  active = null
  fanout = []
  notify(null)
  resolves.forEach((r) => r(pw))
  pump()
}

/** 请求用户输入 sudo 密码；取消返回 null。 */
export function requestSudoPassword(sessionId: string, incorrect: boolean): Promise<string | null> {
  return new Promise((resolve) => {
    // 同 session 的并发请求合并到当前弹窗（一次输入，全部重试）
    if (active && active.sessionId === sessionId) {
      fanout.push(resolve)
      return
    }
    queue.push({ sessionId, incorrect, resolve })
    pump()
  })
}

/**
 * 全局 invoke 包装：捕获 SUDO_PASSWORD_REQUIRED / SUDO_PASSWORD_INCORRECT，
 * 自动弹窗输入 sudo 密码，缓存到会话后重试一次。
 * sessionId 从调用参数中读取（sessionId 或 session_id）。
 */
export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  options?: unknown,
): Promise<T> {
  try {
    return await rawInvoke<T>(cmd, args, options as never)
  } catch (e) {
    const msg = String(e)
    if (msg.includes(SUDO_REQUIRED) || msg.includes(SUDO_INCORRECT)) {
      const sessionId = (args && (args.sessionId || args.session_id)) as string | undefined
      if (!sessionId) throw e
      const pw = await requestSudoPassword(sessionId, msg.includes(SUDO_INCORRECT))
      if (pw !== null && pw.trim() !== '') {
        await rawInvoke('ssh_set_sudo_password', {
          sessionId,
          password: pw,
          configId: null,
          remember: false,
        }).catch(() => {})
        // 会话缓存已设置，重试一次（仅一次，避免循环弹窗）
        return await rawInvoke<T>(cmd, args, options as never)
      }
    }
    throw e
  }
}

/**
 * 兼容旧调用点（透传）：全局 invoke 已统一处理 sudo 弹窗 + 重试，
 * 此处仅保持 API 形态，不再自行弹窗（避免双重弹窗）。
 */
export async function invokeWithSudo<T>(fn: () => Promise<T>, _sessionId?: string): Promise<T> {
  return await fn()
}

/** 全局 sudo 密码弹窗（App 根部渲染一次）。 */
export function SudoPasswordDialog() {
  const { t } = useTranslation()
  const [req, setReq] = useState<SudoRequest | null>(null)
  const [value, setValue] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    const l = (r: SudoRequest | null) => {
      setReq(r)
      setValue('')
      setBusy(false)
    }
    listeners.add(l)
    return () => { listeners.delete(l) }
  }, [])

  if (!req) return null

  const submit = async () => {
    if (value.trim() === '' || busy) return
    setBusy(true)
    settle(value)
  }

  return (
    <div className="sidebar-confirm-overlay">
      <div className="sidebar-edit-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('sudoDialog.title')}</div>
          <button className="sidebar-edit-close" onClick={() => settle(null)}>×</button>
        </div>
        <div className="sidebar-edit-fields">
          <p style={{ margin: '0 0 8px', fontSize: 13, opacity: 0.85 }}>
            {req.incorrect ? t('sudoDialog.incorrect') : t('sudoDialog.description')}
          </p>
          <div className="form-group">
            <label>{t('sudoDialog.password')}</label>
            <input
              className="sidebar-edit-input"
              type="password"
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') submit() }}
              placeholder={t('sudoDialog.passwordPlaceholder')}
            />
          </div>
        </div>
        <div className="sidebar-confirm-actions">
          <button className="sidebar-confirm-btn cancel" onClick={() => settle(null)}>{t('common.cancel')}</button>
          <button className="sidebar-confirm-btn primary" onClick={submit} disabled={busy || value.trim() === ''}>
            {busy ? '...' : t('common.confirm')}
          </button>
        </div>
      </div>
    </div>
  )
}
