import { useState, useEffect, useCallback } from 'react'
import { invoke } from '../../sudoPrompt'
import { useTranslation } from 'react-i18next'
import QRCode from 'qrcode'

interface TfaStatus {
  enabled: boolean
  installed: boolean
  pam_configured: boolean
  sshd_configured: boolean
  secret_initialized: boolean
  light_enabled: boolean
  configurable: boolean
}

interface EnrollResult {
  secret: string
  otpauth_uri: string
  backup_codes: string[]
}

interface TwoFaPanelProps {
  sessionId: string | null
  connId?: string
}

// ===== RFC 6238 TOTP（前端本地校验，无需服务端额外工具） =====
const B32_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'

function base32Decode(input: string): Uint8Array {
  const clean = input.toUpperCase().replace(/=+$/g, '').replace(/\s+/g, '')
  let bits = 0
  let value = 0
  const out: number[] = []
  for (const c of clean) {
    const idx = B32_CHARS.indexOf(c)
    if (idx === -1) continue
    value = (value << 5) | idx
    bits += 5
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 0xff)
      bits -= 8
    }
  }
  return new Uint8Array(out)
}

async function computeTotp(secret: string, timeStep = 30): Promise<string> {
  const key = base32Decode(secret)
  const counter = Math.floor(Date.now() / 1000 / timeStep)
  // 8-byte big-endian counter
  const msg = new Uint8Array(8)
  let c = counter
  for (let i = 7; i >= 0; i--) {
    msg[i] = c & 0xff
    c = Math.floor(c / 256)
  }
  const cryptoKey = await crypto.subtle.importKey(
    'raw', key as BufferSource,
    { name: 'HMAC', hash: 'SHA-1' },
    false, ['sign']
  )
  const sig = await crypto.subtle.sign('HMAC', cryptoKey, msg as BufferSource)
  const h = new Uint8Array(sig)
  const offset = h[h.length - 1] & 0x0f
  const code =
    ((h[offset] & 0x7f) << 24) |
    ((h[offset + 1] & 0xff) << 16) |
    ((h[offset + 2] & 0xff) << 8) |
    (h[offset + 3] & 0xff)
  return String(code % 1000000).padStart(6, '0')
}

// ===== 页面 =====

export default function TwoFaPanel({ sessionId, connId }: TwoFaPanelProps) {
  const { t } = useTranslation()
  const [status, setStatus] = useState<TfaStatus | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState<'install' | 'configure' | 'enroll' | 'disable' | ''>('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  // 开启向导：enroll 后进入"扫码 + 输验证码"阶段，校验通过才标记生效
  const [enrollData, setEnrollData] = useState<EnrollResult | null>(null)
  const [verifyInput, setVerifyInput] = useState('')
  const [qrDataUrl, setQrDataUrl] = useState('')
  const [disableConfirm, setDisableConfirm] = useState(false)

  const refresh = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const s = await invoke<TfaStatus>('tfa_get_status', { sessionId })
      setStatus(s)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => {
    refresh()
  }, [refresh])

  // 生成二维码（otpauth URI）
  useEffect(() => {
    let cancelled = false
    if (enrollData) {
      QRCode.toDataURL(enrollData.otpauth_uri, { width: 180, margin: 1, errorCorrectionLevel: 'M' })
        .then(url => { if (!cancelled) setQrDataUrl(url) })
        .catch(() => {})
    } else {
      setQrDataUrl('')
    }
    return () => { cancelled = true }
  }, [enrollData])

  const runInstall = async () => {
    if (!sessionId) return
    setBusy('install')
    setError('')
    setNotice('')
    try {
      const out = await invoke<string>('tfa_install', { sessionId })
      setNotice(out)
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  const runConfigure = async () => {
    if (!sessionId) return
    setBusy('configure')
    setError('')
    setNotice('')
    try {
      const out = await invoke<string>('tfa_configure', { sessionId })
      setNotice(out)
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  const runEnroll = async () => {
    if (!sessionId) return
    setBusy('enroll')
    setError('')
    setNotice('')
    try {
      const res = await invoke<EnrollResult>('tfa_enroll', { sessionId })
      setEnrollData(res)
      setVerifyInput('')
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  // 开启 2FA：按需 安装 → 配置 → 初始化，全部就绪后进入输码确认阶段
  const handleEnable = async () => {
    if (!sessionId || !status) return
    if (!status.configurable) {
      setError(t('tfa.rootRequired'))
      return
    }
    setError('')
    setNotice('')
    // 1) 依赖
    if (!status.installed) {
      await runInstall()
      if (error) return
    }
    // 2) 配置（PAM / sshd）
    if (!status.pam_configured || !status.sshd_configured) {
      await runConfigure()
      if (error) return
    }
    // 3) 初始化 secret
    if (!status.secret_initialized && !enrollData) {
      await runEnroll()
    }
  }

  // 校验验证码：通过 → 写 tfa_enabled 标记 → 刷新为已生效
  const handleVerify = async () => {
    if (!enrollData || !sessionId) return
    try {
      const expected = await computeTotp(enrollData.secret)
      if (verifyInput.trim() !== expected) {
        setError(t('tfa.codeMismatch'))
        return
      }
    } catch (e) {
      setError(String(e))
      return
    }
    setError('')
    if (connId) {
      await invoke('config_set_tfa_enabled', { configId: connId, enabled: true, tfaType: 'totp' }).catch(() => {})
    }
    setEnrollData(null)
    setVerifyInput('')
    await refresh()
  }

  // 轻量双因素（P4）：强制 publickey,password（密钥+密码），无需 TOTP App
  const handleEnableLight = async () => {
    if (!sessionId || !status) return
    if (!status.configurable) {
      setError(t('tfa.rootRequired'))
      return
    }
    setBusy('configure')
    setError('')
    setNotice('')
    try {
      const out = await invoke<string>('tfa_configure_light', { sessionId })
      setNotice(out)
      if (connId) {
        await invoke('config_set_tfa_enabled', { configId: connId, enabled: true, tfaType: 'keypass' }).catch(() => {})
      }
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  const handleDisable = async () => {
    if (!sessionId) return
    setBusy('disable')
    setError('')
    setNotice('')
    try {
      const out = await invoke<string>('tfa_disable', { sessionId })
      setNotice(out)
      if (connId) {
        await invoke('config_set_tfa_enabled', { configId: connId, enabled: false }).catch(() => {})
      }
      setDisableConfirm(false)
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  if (!sessionId) {
    return <div className="settings-muted">{t('common.connectFirst')}</div>
  }

  const enabled = status?.enabled ?? false
  const configurable = status?.configurable ?? false

  return (
    <div className="settings-card">
      <div className="settings-card-header">{t('nav.2fa')}</div>

      {/* 主开关 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 14 }}>
        <div
          className={`firewall-toggle ${enabled ? 'on' : 'off'} ${loading || busy ? 'loading' : ''}`}
          style={{ cursor: configurable ? 'pointer' : 'not-allowed' }}
          onClick={() => {
            if (loading || busy) return
            if (!configurable) {
              setError(t('tfa.rootRequired'))
              return
            }
            if (!enabled) {
              setDisableConfirm(false)
              handleEnable()
            } else {
              setDisableConfirm(true)
            }
          }}
        >
          <span className="toggle-label">{enabled ? t('common.on') : t('common.off')}</span>
        </div>
        <span style={{ fontSize: 13, color: 'var(--color-text-secondary, #888)' }}>
          {enabled
            ? (status?.light_enabled ? t('tfa.enabledLightHint') : t('tfa.enabledHint'))
            : t('tfa.disabledHint')}
        </span>
      </div>

      {/* 检测状态 */}
      {loading && <div className="settings-muted">{t('settings.loadingAuth')}</div>}

      {status && (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: 14 }}>
          <StatusChip ok={status.installed} label={t('tfa.depInstalled')} />
          <StatusChip ok={status.pam_configured} label={t('tfa.pamConfigured')} />
          <StatusChip ok={status.sshd_configured} label={t('tfa.sshdConfigured')} />
          <StatusChip ok={status.secret_initialized} label={t('tfa.secretReady')} />
        </div>
      )}

      {!configurable && (
        <div className="settings-warning" style={{ marginBottom: 12 }}>{t('tfa.rootRequired')}</div>
      )}

      {/* 开启向导：安装 / 配置 / 初始化（TOTP 模式） */}
      {!enabled && configurable && !enrollData && !status?.light_enabled && (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: 12 }}>
          {!status?.installed && (
            <button className="sidebar-confirm-btn primary" disabled={busy === 'install'} onClick={runInstall}>
              {busy === 'install' ? t('tfa.installing') : t('tfa.install')}
            </button>
          )}
          {(!status?.pam_configured || !status?.sshd_configured) && (
            <button className="sidebar-confirm-btn primary" disabled={busy === 'configure'} onClick={runConfigure}>
              {busy === 'configure' ? t('tfa.configuring') : t('tfa.configure')}
            </button>
          )}
          {!status?.secret_initialized && (
            <button className="sidebar-confirm-btn primary" disabled={busy === 'enroll'} onClick={runEnroll}>
              {busy === 'enroll' ? t('tfa.enrolling') : t('tfa.enroll')}
            </button>
          )}
        </div>
      )}

      {/* 轻量双因素（P4）：密钥 + 密码 */}
      {!enabled && configurable && !status?.light_enabled && (
        <div style={{ marginBottom: 12, padding: 10, borderRadius: 8, border: '1px solid var(--color-border-tertiary, #eee)' }}>
          <div style={{ fontSize: 13, fontWeight: 500, marginBottom: 4 }}>{t('tfa.lightTitle')}</div>
          <div className="settings-muted" style={{ marginBottom: 8 }}>{t('tfa.lightDesc')}</div>
          <button className="sidebar-confirm-btn primary" disabled={busy === 'configure'} onClick={handleEnableLight}>
            {busy === 'configure' ? t('tfa.configuring') : t('tfa.enableLight')}
          </button>
        </div>
      )}

      {/* 开启向导：扫码 + 输验证码确认 */}
      {enrollData && (
        <div style={{ marginBottom: 12 }}>
          <div className="settings-section-sub-header">{t('tfa.scanStep')}</div>
          <div style={{ display: 'flex', gap: 16, alignItems: 'flex-start', marginTop: 8 }}>
            <div style={{ flexShrink: 0 }}>
              {qrDataUrl ? (
                <img src={qrDataUrl} alt="TOTP QR" style={{ width: 180, height: 180, borderRadius: 8, border: '1px solid var(--color-border-tertiary, #ddd)' }} />
              ) : (
                <div style={{ width: 180, height: 180, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#999', fontSize: 12 }}>QR…</div>
              )}
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="settings-muted" style={{ marginBottom: 6 }}>{t('tfa.manualKey')}</div>
              <code style={{ userSelect: 'all', wordBreak: 'break-all', fontSize: 13, background: 'var(--color-background-secondary, #f5f5f5)', padding: '6px 8px', borderRadius: 6, display: 'inline-block' }}>
                {enrollData.secret}
              </code>
              {enrollData.backup_codes.length > 0 && (
                <>
                  <div className="settings-muted" style={{ marginTop: 10, marginBottom: 4 }}>{t('tfa.backupCodes')}</div>
                  <div style={{ fontFamily: 'monospace', fontSize: 13, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                    {enrollData.backup_codes.map((c, i) => (
                      <span key={i} style={{ background: 'var(--color-background-secondary, #f5f5f5)', padding: '2px 6px', borderRadius: 4 }}>{c}</span>
                    ))}
                  </div>
                </>
              )}
              <div style={{ marginTop: 12, display: 'flex', gap: 8, alignItems: 'center' }}>
                <input
                  className="sidebar-edit-input"
                  style={{ width: 130, textAlign: 'center', letterSpacing: 4, fontSize: 16 }}
                  value={verifyInput}
                  onChange={(e) => setVerifyInput(e.target.value.replace(/\D/g, '').slice(0, 6))}
                  placeholder="••••••"
                  autoComplete="off"
                  onKeyDown={(e) => { if (e.key === 'Enter' && verifyInput.length === 6) handleVerify() }}
                />
                <button className="sidebar-confirm-btn primary" disabled={verifyInput.length !== 6} onClick={handleVerify}>
                  {t('tfa.verifyAndEnable')}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* 关闭 2FA（二次确认） */}
      {enabled && (
        <div style={{ marginBottom: 12 }}>
          {disableConfirm ? (
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', padding: 10, borderRadius: 8, background: 'var(--color-background-danger, #fcebeb)', border: '1px solid var(--color-border-danger, #f09595)' }}>
              <span style={{ fontSize: 13 }}>{t('tfa.disableConfirmMsg')}</span>
              <button className="sidebar-confirm-btn danger" disabled={busy === 'disable'} onClick={handleDisable}>
                {busy === 'disable' ? t('tfa.disabling') : t('tfa.confirmDisable')}
              </button>
              <button className="sidebar-confirm-btn cancel" onClick={() => setDisableConfirm(false)}>{t('common.cancel')}</button>
            </div>
          ) : (
            <button className="sidebar-confirm-btn danger" onClick={() => setDisableConfirm(true)}>{t('tfa.disable')}</button>
          )}
        </div>
      )}

      {/* 反馈 */}
      {error && <div style={{ color: 'var(--color-text-danger, #a32d2d)', fontSize: 13, marginBottom: 8 }}>⚠ {error}</div>}
      {notice && <div className="settings-muted" style={{ marginBottom: 8 }}>{notice}</div>}

      {/* 防锁死兜底提示 */}
      <div style={{ fontSize: 12, lineHeight: 1.7, color: 'var(--color-text-tertiary, #999)', borderTop: '1px solid var(--color-border-tertiary, #eee)', paddingTop: 10 }}>
        <div style={{ fontWeight: 500, marginBottom: 4 }}>{t('tfa.safetyTitle')}</div>
        <div>{t('tfa.safetyHint1')}</div>
        <div>{t('tfa.safetyHint2')}</div>
        <div>{t('tfa.safetyHint3')}</div>
      </div>
    </div>
  )
}

function StatusChip({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span style={{
      fontSize: 12,
      padding: '3px 10px',
      borderRadius: 999,
      border: '1px solid',
      borderColor: ok ? 'var(--color-border-success, #97c459)' : 'var(--color-border-tertiary, #ddd)',
      background: ok ? 'var(--color-background-success, #eaf3de)' : 'transparent',
      color: ok ? 'var(--color-text-success, #3b6d11)' : 'var(--color-text-tertiary, #999)',
    }}>
      {ok ? '✓ ' : '✗ '}{label}
    </span>
  )
}
