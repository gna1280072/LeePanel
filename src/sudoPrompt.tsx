import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'

// 权限模型 v8：sudo 密码交互（ask 模式）
// - 后端 exec 层在需要 sudo 但会话无密码时返回约定错误码
// - invokeWithSudo 捕获后弹出全局输入框，缓存到会话后自动重试一次

export const SUDO_REQUIRED = 'SUDO_PASSWORD_REQUIRED'
export const SUDO_INCORRECT = 'SUDO_PASSWORD_INCORRECT'

interface SudoRequest {
  sessionId: string
  incorrect: boolean
  resolve: (pw: string | null) => void
}

let currentRequest: SudoRequest | null = null
const listeners = new Set<(r: SudoRequest | null) => void>()

function notify(r: SudoRequest | null) {
  listeners.forEach((l) => l(r))
}

/** 请求用户输入 sudo 密码；取消返回 null。 */
export function requestSudoPassword(sessionId: string, incorrect: boolean): Promise<string | null> {
  return new Promise((resolve) => {
    currentRequest = {
      sessionId,
      incorrect,
      resolve: (pw) => {
        currentRequest = null
        notify(null)
        resolve(pw)
      },
    }
    notify(currentRequest)
  })
}

/**
 * 包装一次远程调用：若返回 SUDO_PASSWORD_REQUIRED / SUDO_PASSWORD_INCORRECT，
 * 弹出 sudo 密码输入框，缓存到会话后自动重试一次。
 */
export async function invokeWithSudo<T>(fn: () => Promise<T>, sessionId: string): Promise<T> {
  try {
    return await fn()
  } catch (e) {
    const msg = String(e)
    if (msg.includes(SUDO_REQUIRED) || msg.includes(SUDO_INCORRECT)) {
      const pw = await requestSudoPassword(sessionId, msg.includes(SUDO_INCORRECT))
      if (pw !== null && pw.trim() !== '') {
        await invoke('ssh_set_sudo_password', {
          sessionId,
          password: pw,
          configId: null,
          remember: false,
        }).catch(() => {})
        // 会话缓存已设置，重试一次（仅一次，避免循环弹窗）
        return await fn()
      }
    }
    throw e
  }
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
    req.resolve(value)
  }

  return (
    <div className="sidebar-confirm-overlay">
      <div className="sidebar-edit-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('sudoDialog.title')}</div>
          <button className="sidebar-edit-close" onClick={() => req.resolve(null)}>×</button>
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
          <button className="sidebar-confirm-btn cancel" onClick={() => req.resolve(null)}>{t('common.cancel')}</button>
          <button className="sidebar-confirm-btn primary" onClick={submit} disabled={busy || value.trim() === ''}>
            {busy ? '...' : t('common.confirm')}
          </button>
        </div>
      </div>
    </div>
  )
}
