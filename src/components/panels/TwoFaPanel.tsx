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

// ===== 页面状态机 =====
// status        状态视图（未开启 → 模式卡片；已开启 → 管理操作）
// wizard-totp   TOTP 三步向导：prepare（环境准备，自动串行）→ bind（扫码绑定）→ verify（验证生效）→ done
// wizard-light  轻量双因素单步确认
type ViewState = 'status' | 'wizard-totp' | 'wizard-light'
type TotpStep = 'prepare' | 'bind' | 'verify' | 'done'
type StepState = 'pending' | 'running' | 'done' | 'failed' | 'skipped'

const STEP_ICON: Record<StepState, string> = {
  pending: '○', running: '↻', done: '✓', failed: '✗', skipped: '✓',
}

export default function TwoFaPanel({ sessionId, connId }: TwoFaPanelProps) {
  const { t } = useTranslation()
  const [view, setView] = useState<ViewState>('status')
  const [status, setStatus] = useState<TfaStatus | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')

  // TOTP 向导
  const [totpStep, setTotpStep] = useState<TotpStep>('prepare')
  const [installState, setInstallState] = useState<StepState>('pending')
  const [configState, setConfigState] = useState<StepState>('pending')
  const [enrollData, setEnrollData] = useState<EnrollResult | null>(null)
  const [existingSecret, setExistingSecret] = useState(false)
  const [savedBackup, setSavedBackup] = useState(false)
  const [verifyInput, setVerifyInput] = useState('')
  const [qrDataUrl, setQrDataUrl] = useState('')

  // 管理操作 / 弹层
  const [busy, setBusy] = useState('')
  const [viewBackups, setViewBackups] = useState<EnrollResult | null>(null)
  const [regenConfirm, setRegenConfirm] = useState(false)
  const [disableConfirm, setDisableConfirm] = useState(false)
  const [cancelConfirm, setCancelConfirm] = useState(false)
  const [diagOpen, setDiagOpen] = useState(false)

  const refresh = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
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

  // 二维码（otpauth URI → data URL）
  useEffect(() => {
    let cancelled = false
    if (enrollData) {
      QRCode.toDataURL(enrollData.otpauth_uri, { width: 180, margin: 1, errorCorrectionLevel: 'M' })
        .then(url => { if (!cancelled) setQrDataUrl(url) })
        .catch(e => { if (!cancelled) setError(`QR error: ${e}`) })
    } else {
      setQrDataUrl('')
    }
    return () => { cancelled = true }
  }, [enrollData])

  const doEnroll = async () => {
    if (!sessionId) return
    setBusy('enroll')
    setError('')
    try {
      const res = await invoke<EnrollResult>('tfa_enroll', { sessionId })
      setExistingSecret(false)
      setEnrollData(res)
      setSavedBackup(false)
      setVerifyInput('')
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  // 环境准备：自动串行 安装 → 配置，每步可视进度；失败停住可重试
  const runPrepare = async () => {
    if (!sessionId) return
    setError('')
    try {
      const s = await invoke<TfaStatus>('tfa_get_status', { sessionId })
      setStatus(s)
      const needsInstall = !s.installed
      const needsConfig = !(s.pam_configured && s.sshd_configured)
      setInstallState(needsInstall ? 'running' : 'skipped')
      setConfigState(needsConfig ? 'running' : 'skipped')
      if (needsInstall) {
        try {
          await invoke<string>('tfa_install', { sessionId })
          setInstallState('done')
        } catch (e) {
          setInstallState('failed')
          setError(String(e))
          return
        }
      }
      if (needsConfig) {
        try {
          await invoke<string>('tfa_configure', { sessionId })
          setConfigState('done')
        } catch (e) {
          setConfigState('failed')
          setError(String(e))
          return
        }
      }
      // 进入扫码绑定：先探测服务器是否已有 secret（向导中途退出后重进 / 手动配置过）
      setTotpStep('bind')
      try {
        await invoke<EnrollResult>('tfa_read_secret', { sessionId })
        setExistingSecret(true)
      } catch {
        await doEnroll()
      }
    } catch (e) {
      setError(String(e))
    }
  }

  const startWizard = async () => {
    if (!sessionId || !status) return
    if (!status.configurable) {
      setError(t('tfa.rootRequired'))
      return
    }
    setEnrollData(null)
    setExistingSecret(false)
    setSavedBackup(false)
    setVerifyInput('')
    setError('')
    setNotice('')
    setTotpStep('prepare')
    setView('wizard-totp')
    await runPrepare()
  }

  const startLight = () => {
    if (!status?.configurable) {
      setError(t('tfa.rootRequired'))
      return
    }
    setError('')
    setNotice('')
    setView('wizard-light')
  }

  const confirmLight = async () => {
    if (!sessionId) return
    setBusy('configure')
    setError('')
    try {
      await invoke<string>('tfa_configure_light', { sessionId })
      if (connId) {
        await invoke('config_set_tfa_enabled', { configId: connId, enabled: true, tfaType: 'keypass' }).catch(() => {})
      }
      window.dispatchEvent(new CustomEvent('tfa-status-changed'))
      setView('status')
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  // 校验验证码：通过 → 标记生效 → 成功反馈 → 回状态视图
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
    // 通知 App 刷新 Sidebar 连接列表（tfa_enabled 标记即时同步）
    window.dispatchEvent(new CustomEvent('tfa-status-changed'))
    setTotpStep('done')
    setTimeout(() => {
      setView('status')
      setEnrollData(null)
      setVerifyInput('')
      refresh()
    }, 1600)
  }

  const handleViewBackups = async () => {
    if (!sessionId) return
    setBusy('backup')
    setError('')
    try {
      const res = await invoke<EnrollResult>('tfa_read_secret', { sessionId })
      setViewBackups(res)
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy('')
    }
  }

  // 重新生成密钥：二次确认后 enroll → 直接进向导 bind 步（环境已就绪）
  const handleRegen = async () => {
    setRegenConfirm(false)
    setTotpStep('bind')
    setView('wizard-totp')
    await doEnroll()
  }

  const handleDisable = async () => {
    if (!sessionId) return
    setBusy('disable')
    setError('')
    setNotice('')
    try {
      await invoke<string>('tfa_disable', { sessionId })
      if (connId) {
        await invoke('config_set_tfa_enabled', { configId: connId, enabled: false }).catch(() => {})
      }
      // 通知 App 刷新 Sidebar 连接列表（tfa_enabled 即时同步，避免"已关闭仍弹验证码"）
      window.dispatchEvent(new CustomEvent('tfa-status-changed'))
      setDisableConfirm(false)
      setView('status')
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
  const lightOn = status?.light_enabled ?? false
  const configurable = status?.configurable ?? false

  return (
    <div className="settings-card">
      <div className="settings-card-header">{t('nav.2fa')}</div>

      {/* ==================== 状态视图 ==================== */}
      {view === 'status' && (
        <>
          <div style={{ padding: '16px 16px 0' }}>
            {/* 状态卡 */}
            <div className="tfa-status">
              <div className={`tfa-status-icon ${enabled ? 'on' : 'off'}`}>{enabled ? '✓' : '🔐'}</div>
              <div style={{ flex: 1 }}>
                <div className="tfa-status-title">
                  {enabled ? (lightOn ? t('tfa.enabledLightHint') : t('tfa.totpOn')) : t('tfa.disabledHint')}
                </div>
                <div className="tfa-status-sub">{t('tfa.sessionSafe')}</div>
              </div>
              {loading && <span className="tfa-status-spin">↻</span>}
            </div>

            {!configurable && (
              <div className="tfa-warn-line">⚠ {t('tfa.rootRequired')}</div>
            )}

            {/* 未开启 → 模式选择卡片 */}
            {!enabled && (
              <div className="tfa-mode-grid">
                <div className="tfa-mode-card feat">
                  <span className="tfa-badge reco">{t('tfa.badgeRecommended')}</span>
                  <div className="tfa-mode-title">{t('tfa.modeTotp')}</div>
                  <div className="tfa-mode-desc">{t('tfa.modeTotpDesc')}</div>
                  <button className="sidebar-confirm-btn primary" onClick={startWizard} disabled={loading}>
                    {t('tfa.startWizard')}
                  </button>
                </div>
                <div className="tfa-mode-card">
                  <span className="tfa-badge light">{t('tfa.badgeLight')}</span>
                  <div className="tfa-mode-title">{t('tfa.lightTitle')}</div>
                  <div className="tfa-mode-desc">{t('tfa.lightDesc')}</div>
                  <button className="sidebar-confirm-btn" onClick={startLight}>{t('tfa.enableLight')}</button>
                </div>
              </div>
            )}

            {/* 已开启 → 管理操作 */}
            {enabled && (
              <div className="tfa-actions">
                {!lightOn && (
                  <>
                    <button className="sidebar-confirm-btn" disabled={busy === 'backup'} onClick={handleViewBackups}>
                      {busy === 'backup' ? '↻' : '👁'} {t('tfa.viewBackupCodes')}
                    </button>
                    <button className="sidebar-confirm-btn" onClick={() => setRegenConfirm(true)}>{t('tfa.regenKey')}</button>
                  </>
                )}
                {disableConfirm ? (
                  <span className="tfa-alert danger" style={{ display: 'inline-flex', gap: 8, alignItems: 'center', marginBottom: 0 }}>
                    <span>{t('tfa.disableConfirmMsg')}</span>
                    <button className="sidebar-confirm-btn danger" disabled={busy === 'disable'} onClick={handleDisable}>
                      {busy === 'disable' ? t('tfa.disabling') : t('tfa.confirmDisable')}
                    </button>
                    <button className="sidebar-confirm-btn cancel" onClick={() => setDisableConfirm(false)}>{t('common.cancel')}</button>
                  </span>
                ) : (
                  <button className="sidebar-confirm-btn danger" onClick={() => setDisableConfirm(true)}>{t('tfa.disable')}</button>
                )}
              </div>
            )}
          </div>

          {/* 诊断信息（折叠） */}
          <div className="tfa-diag" style={{ paddingLeft: 16, paddingRight: 16 }}>
            <div className="tfa-diag-toggle" onClick={() => setDiagOpen(!diagOpen)}>
              {diagOpen ? '▾' : '▸'} {t('tfa.diagInfo')}
            </div>
            {diagOpen && status && (
              <div className="tfa-diag-body">
                <div>
                  <span className={`tfa-chip ${status.installed ? 'ok' : 'no'}`}>{t('tfa.depInstalled')}</span>
                  <span className={`tfa-chip ${status.pam_configured ? 'ok' : 'no'}`}>{t('tfa.pamConfigured')}</span>
                  <span className={`tfa-chip ${status.sshd_configured ? 'ok' : 'no'}`}>{t('tfa.sshdConfigured')}</span>
                  <span className={`tfa-chip ${status.secret_initialized ? 'ok' : 'no'}`}>{t('tfa.secretReady')}</span>
                </div>
                <div className="tfa-diag-note">
                  <div>{t('tfa.safetyHint1')}</div>
                  <div>{t('tfa.safetyHint2')}</div>
                </div>
              </div>
            )}
          </div>
        </>
      )}

      {/* ==================== TOTP 向导 ==================== */}
      {view === 'wizard-totp' && (
        <div style={{ padding: '16px 16px 0' }}>
          {/* stepper */}
          <div className="tfa-stepper">
            <StepDot state={totpStep === 'prepare' ? 'active' : 'done'} label={t('tfa.stepPrepare')} step={1} />
            <div className="tfa-step-line" />
            <StepDot state={totpStep === 'bind' ? 'active' : ['verify', 'done'].includes(totpStep) ? 'done' : 'todo'} label={t('tfa.stepBind')} step={2} />
            <div className="tfa-step-line" />
            <StepDot state={totpStep === 'verify' ? 'active' : totpStep === 'done' ? 'done' : 'todo'} label={t('tfa.stepVerify')} step={3} />
          </div>

          {/* Step 1 环境准备 */}
          {totpStep === 'prepare' && (
            <div>
              <PrepareRow label={t('tfa.stepInstallLabel')} state={installState} statusText={t('tfa.running')} failedText={t('tfa.stepFailedLabel')} skippedText={t('tfa.stepSkipped')} />
              <PrepareRow label={t('tfa.stepConfigLabel')} state={configState} statusText={t('tfa.running')} failedText={t('tfa.stepFailedLabel')} skippedText={t('tfa.stepSkipped')} />
              {(installState === 'failed' || configState === 'failed') && (
                <div style={{ marginTop: 12 }}>
                  <button className="sidebar-confirm-btn primary" onClick={runPrepare}>{t('tfa.stepRetry')}</button>
                </div>
              )}
            </div>
          )}

          {/* Step 2 扫码绑定 */}
          {totpStep === 'bind' && (
            <div>
              {/* 已有密钥选择 */}
              {existingSecret && !enrollData && (
                <div className="tfa-alert warn">
                  <div className="t">{t('tfa.existingSecret')}</div>
                  <div>{t('tfa.existingSecretHint')}</div>
                  <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
                    <button className="sidebar-confirm-btn primary" onClick={async () => {
                      if (!sessionId) return
                      setBusy('backup')
                      try {
                        const res = await invoke<EnrollResult>('tfa_read_secret', { sessionId })
                        setExistingSecret(false)
                        setEnrollData(res)
                        setSavedBackup(false)
                      } catch (e) { setError(String(e)) } finally { setBusy('') }
                    }}>{t('tfa.useExisting')}</button>
                    <button className="sidebar-confirm-btn" onClick={doEnroll} disabled={busy === 'enroll'}>{t('tfa.regenKey')}</button>
                  </div>
                </div>
              )}

              {enrollData && (
                <div className="tfa-bind">
                  <div className="tfa-qr-wrap">
                    {qrDataUrl ? (
                      <img src={qrDataUrl} alt="TOTP QR" className="tfa-qr-img" />
                    ) : (
                      <div className="tfa-qr-ph">QR…</div>
                    )}
                  </div>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="tfa-secret-label">{t('tfa.manualKey')}</div>
                    <code className="tfa-secret-box">{enrollData.secret}</code>
                    {enrollData.backup_codes.length > 0 && (
                      <>
                        <div className="tfa-backup-label">{t('tfa.backupCodes')}</div>
                        <div className="tfa-backup-chips">
                          {enrollData.backup_codes.map((c, i) => (
                            <span key={i} className="tfa-backup-chip">{c}</span>
                          ))}
                        </div>
                      </>
                    )}
                    <label className="tfa-saved-row">
                      <input type="checkbox" checked={savedBackup} onChange={(e) => setSavedBackup(e.target.checked)} />
                      {t('tfa.savedBackupConfirm')}
                    </label>
                  </div>
                </div>
              )}

              <div className="tfa-wizard-foot">
                {cancelConfirm ? (
                  <span style={{ display: 'inline-flex', gap: 8, alignItems: 'center', fontSize: 12 }}>
                    <span>{t('tfa.cancelWizardMsg')}</span>
                    <button className="sidebar-confirm-btn danger" onClick={() => { setCancelConfirm(false); setView('status') }}>{t('common.confirm')}</button>
                    <button className="sidebar-confirm-btn cancel" onClick={() => setCancelConfirm(false)}>{t('common.cancel')}</button>
                  </span>
                ) : (
                  <button className="sidebar-confirm-btn cancel" onClick={() => setCancelConfirm(true)}>{t('common.cancel')}</button>
                )}
                {enrollData && (
                  <button className="sidebar-confirm-btn primary" disabled={!savedBackup} onClick={() => setTotpStep('verify')}>
                    {t('tfa.nextVerify')}
                  </button>
                )}
              </div>
            </div>
          )}

          {/* Step 3 验证生效 */}
          {totpStep === 'verify' && enrollData && (
            <div>
              <div className="settings-muted" style={{ marginBottom: 10 }}>{t('tfa.verifyHint')}</div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input
                  className="sidebar-edit-input"
                  style={{ width: 140, textAlign: 'center', letterSpacing: 4, fontSize: 16 }}
                  value={verifyInput}
                  onChange={(e) => setVerifyInput(e.target.value.replace(/\D/g, '').slice(0, 6))}
                  placeholder="••••••"
                  autoFocus
                  autoComplete="off"
                  onKeyDown={(e) => { if (e.key === 'Enter' && verifyInput.length === 6) handleVerify() }}
                />
                <button className="sidebar-confirm-btn primary" disabled={verifyInput.length !== 6} onClick={handleVerify}>
                  {t('tfa.verifyAndEnable')}
                </button>
                <button className="sidebar-confirm-btn cancel" onClick={() => setTotpStep('bind')}>{t('tfa.back')}</button>
              </div>
            </div>
          )}

          {/* 完成 */}
          {totpStep === 'done' && (
            <div className="tfa-success">
              <span className="tfa-success-check">✓</span>
              <span className="tfa-success-text">{t('tfa.enabledSuccess')}</span>
            </div>
          )}
        </div>
      )}

      {/* ==================== 轻量模式确认页 ==================== */}
      {view === 'wizard-light' && (
        <div style={{ padding: '16px 16px 0' }}>
          <div className="tfa-alert info">
            <div className="t">{t('tfa.lightTitle')}</div>
            <div>{t('tfa.lightDesc')}</div>
          </div>
          <div className="settings-muted" style={{ marginBottom: 16 }}>{t('tfa.safetyHint1')}</div>
          <div className="tfa-wizard-foot">
            <button className="sidebar-confirm-btn cancel" onClick={() => setView('status')}>{t('common.cancel')}</button>
            <button className="sidebar-confirm-btn primary" disabled={busy === 'configure'} onClick={confirmLight}>
              {busy === 'configure' ? t('tfa.configuring') : t('tfa.lightConfirm')}
            </button>
          </div>
        </div>
      )}

      {/* 重新生成密钥确认 */}
      {regenConfirm && (
        <div style={{ padding: '0 16px 14px' }}>
          <div className="tfa-alert danger">
            <div>{t('tfa.regenConfirmMsg')}</div>
            <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
              <button className="sidebar-confirm-btn danger" onClick={handleRegen}>{t('common.confirm')}</button>
              <button className="sidebar-confirm-btn cancel" onClick={() => setRegenConfirm(false)}>{t('common.cancel')}</button>
            </div>
          </div>
        </div>
      )}

      {/* 查看备用码 */}
      {viewBackups && (
        <div style={{ padding: '0 16px 14px' }}>
          <div className="tfa-alert info">
            <div className="t">{t('tfa.viewBackupCodes')}</div>
            {viewBackups.backup_codes.length > 0 ? (
              <div className="tfa-backup-chips" style={{ marginTop: 8 }}>
                {viewBackups.backup_codes.map((c, i) => (
                  <span key={i} className="tfa-backup-chip">{c}</span>
                ))}
              </div>
            ) : (
              <div>{t('tfa.noBackupCodes')}</div>
            )}
            <div style={{ marginTop: 10 }}>
              <button className="sidebar-confirm-btn cancel" onClick={() => setViewBackups(null)}>{t('common.close')}</button>
            </div>
          </div>
        </div>
      )}

      {/* 反馈 */}
      {error && <div className="tfa-error">⚠ {error}</div>}
      {notice && <div className="tfa-notice">{notice}</div>}
    </div>
  )
}

function StepDot({ state, label, step }: { state: 'todo' | 'active' | 'done'; label: string; step: number }) {
  const dotClass = state === 'done' ? 'done' : state === 'active' ? 'active' : 'todo'
  return (
    <div className={`tfa-step ${state === 'active' ? 'active' : ''}`}>
      <span className={`tfa-step-dot ${dotClass}`}>{state === 'done' ? '✓' : step}</span>
      <span>{label}</span>
    </div>
  )
}

function PrepareRow({ label, state, statusText, failedText, skippedText }: {
  label: string
  state: StepState
  statusText: string
  failedText: string
  skippedText: string
}) {
  const stateClass = state === 'done' || state === 'skipped' ? 'done'
    : state === 'failed' ? 'failed'
    : state === 'running' ? 'running' : 'todo'
  const status = state === 'running' ? statusText
    : state === 'failed' ? failedText
    : state === 'skipped' ? skippedText : ''
  return (
    <div className="tfa-prepare-row">
      <span className={`tfa-prepare-state ${stateClass}`}>{STEP_ICON[state]}</span>
      <span className="tfa-prepare-name">{label}</span>
      <span className="tfa-prepare-status">{status}</span>
    </div>
  )
}
