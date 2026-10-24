import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '../../sudoPrompt'
import { listen, emit } from '@tauri-apps/api/event'
import { open as openUrl } from '@tauri-apps/plugin-shell'
import { open as openFile } from '@tauri-apps/plugin-dialog'
import { readTextFile } from '@tauri-apps/plugin-fs'
import { useTranslation } from 'react-i18next'

interface SiteInfo {
  domain: string
  domains: string
  root: string
  config_path: string
  ssl: boolean
  ssl_cert_path?: string
  ssl_key_path?: string
  php_version: string
  enabled: boolean
  created_at: number
}

interface SslCertInfo {
  domain: string
  has_cert: boolean
  source: string
  cert_path: string
  key_path: string
  subject: string
  issuer: string
  not_before: string
  not_after: string
  days_remaining: number
  sans: string[]
  key_info: string
  fingerprint: string
  chain_length: number
}

interface SslCheckItem {
  code: string
  level: 'ok' | 'warn' | 'error'
  detail: string
  days: number
}

interface SslPrecheck {
  ok: boolean
  items: SslCheckItem[]
  info: SslCertInfo | null
}

interface SslPanelProps {
  sessionId: string | null
}

interface SslLog {
  domain: string
  lines: string[]
  status: 'idle' | 'installing' | 'done' | 'error'
}

const PEM_FILTERS = [{ name: 'PEM', extensions: ['pem', 'crt', 'cer', 'key', 'txt'] }]

export default function SslPanel({ sessionId }: SslPanelProps) {
  const { t } = useTranslation()
  const [sites, setSites] = useState<SiteInfo[]>([])
  const [infos, setInfos] = useState<Record<string, SslCertInfo>>({})
  const [loading, setLoading] = useState(false)
  const [logs, setLogs] = useState<Record<string, SslLog>>({})
  const logEndRef = useRef<HTMLDivElement>(null)

  // 弹窗状态
  const [uploadFor, setUploadFor] = useState<SiteInfo | null>(null)
  const [selfSignedFor, setSelfSignedFor] = useState<SiteInfo | null>(null)
  const [detail, setDetail] = useState<SslCertInfo | null>(null)
  const [removeFor, setRemoveFor] = useState<SiteInfo | null>(null)
  const [removeInput, setRemoveInput] = useState('')

  const fetchAll = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    try {
      const list = await invoke<SiteInfo[]>('server_list_sites', { sessionId })
      setSites(list)
      const sslSites = list.filter(s => s.ssl && s.config_path)
      if (sslSites.length > 0) {
        const result = await invoke<SslCertInfo[]>('server_ssl_site_infos', {
          sessionId,
          targets: sslSites.map(s => ({ domain: s.domain, config_path: s.config_path })),
        })
        const map: Record<string, SslCertInfo> = {}
        for (const info of result) map[info.domain] = info
        setInfos(map)
      } else {
        setInfos({})
      }
    } catch {
      // ignore
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => { fetchAll() }, [fetchAll])

  // Listen for ssl-install-progress events
  useEffect(() => {
    if (!sessionId) return
    const unlisten = listen<{ sessionId: string; domain: string; line: string; status: string }>(
      'ssl-install-progress',
      (event) => {
        if (event.payload.sessionId !== sessionId) return
        const { domain, line, status } = event.payload
        setLogs(prev => {
          const entry = prev[domain] || { domain, lines: [], status: 'idle' }
          return {
            ...prev,
            [domain]: {
              ...entry,
              lines: [...entry.lines, line],
              status: status as SslLog['status'],
            }
          }
        })
      }
    )
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId])

  // Listen for ssl-installed events to refresh site list
  useEffect(() => {
    if (!sessionId) return
    const unlisten = listen<{ sessionId: string; domain: string }>(
      'ssl-installed',
      (event) => {
        if (event.payload.sessionId === sessionId) {
          fetchAll()
        }
      }
    )
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId, fetchAll])

  // Auto-scroll log to bottom
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [logs])

  const handleInstall = async (domain: string) => {
    if (!sessionId) return
    setLogs(prev => ({
      ...prev,
      [domain]: { domain, lines: [t('ssl.startingSetup', { domain })], status: 'installing' }
    }))
    try {
      await invoke<string>('server_setup_ssl', { sessionId, domain })
      await emit('ssl-installed', { sessionId, domain })
    } catch (e) {
      setLogs(prev => {
        const entry = prev[domain] || { domain, lines: [], status: 'idle' }
        return {
          ...prev,
          [domain]: { ...entry, lines: [...entry.lines, String(e)], status: 'error' }
        }
      })
    }
  }

  if (!sessionId) {
    return <div className="sp-empty">{t('ssl.connectToManage')}</div>
  }

  const activeLog = Object.values(logs).find(l => l.status === 'installing')
    || Object.values(logs).find(l => l.status !== 'idle')

  const sourceLabel = (source: string) => {
    switch (source) {
      case 'letsencrypt': return t('ssl.sourceLetsEncrypt')
      case 'custom': return t('ssl.sourceCustom')
      case 'selfsigned': return t('ssl.sourceSelfSigned')
      case 'other': return t('ssl.sourceOther')
      default: return t('ssl.sourceNone')
    }
  }

  const expiringClass = (info?: SslCertInfo) => {
    if (!info || !info.has_cert) return ''
    if (info.days_remaining < 0) return ''
    if (info.days_remaining <= 0) return 'expired'
    if (info.days_remaining <= 30) return 'soon'
    return ''
  }

  return (
    <div className="ssl-panel">
      <div className="sp-section-header">
        <h3>{t('ssl.title')}</h3>
        <button className="svc-cfg-btn" onClick={fetchAll} disabled={loading}>
          {loading ? t('common.loading') : t('common.refresh')}
        </button>
      </div>

      {sites.length === 0 && !loading && (
        <div className="sp-empty">{t('ssl.noSites')}</div>
      )}

      <div className="sites-grid">
        {sites.map(site => {
          const info = infos[site.domain]
          const hasCert = !!(info && info.has_cert) || site.ssl
          const managedByPanel = !!info && (info.source === 'custom' || info.source === 'selfsigned')
          const busy = logs[site.domain]?.status === 'installing'
          const expiryClass = expiringClass(info)
          return (
            <div key={site.domain} className={`site-card ${site.ssl ? 'running' : ''}`}>
              <div className="site-card-header">
                <div className="site-domain">
                  <span className="site-domain-text">{site.domain}</span>
                  {site.ssl ? (
                    <span className="site-ssl-badge">SSL</span>
                  ) : (
                    <span className="site-ssl-badge no-ssl">{t('ssl.noSsl')}</span>
                  )}
                </div>
              </div>

              <div className="site-card-body">
                <div className="site-info-row">
                  <span className="site-info-value mono site-root-link">{site.root}</span>
                </div>
                {info && info.has_cert && (
                  <>
                    <div className="site-info-row">
                      <span className="site-info-label">{t('ssl.source')}</span>
                      <span className={`ssl-source-badge ${info.source}`}>
                        {sourceLabel(info.source)}
                      </span>
                    </div>
                    <div className="site-info-row">
                      <span className="site-info-label">{t('ssl.expiresOn')}</span>
                      <span className={`site-info-value ssl-expiry ${expiryClass}`}>
                        {info.days_remaining >= 0
                          ? t('ssl.daysLeft', { days: info.days_remaining })
                          : info.not_after || t('ssl.daysLeftUnknown')}
                      </span>
                    </div>
                  </>
                )}
              </div>

              <div className="site-card-actions">
                {!site.ssl && (
                  <button
                    className="svc-cfg-btn primary"
                    onClick={() => handleInstall(site.domain)}
                    disabled={busy}
                  >
                    {busy ? t('ssl.installing') : t('ssl.freeInstall')}
                  </button>
                )}
                <button
                  className="svc-cfg-btn"
                  onClick={() => setUploadFor(site)}
                  disabled={busy}
                >
                  {hasCert ? t('ssl.replaceCert') : t('ssl.uploadCustom')}
                </button>
                {!site.ssl && (
                  <button
                    className="svc-cfg-btn"
                    onClick={() => setSelfSignedFor(site)}
                    disabled={busy}
                  >
                    {t('ssl.generateSelfSigned')}
                  </button>
                )}
                {info && info.has_cert && (
                  <button className="svc-cfg-btn" onClick={() => setDetail(info)}>
                    {t('ssl.viewCert')}
                  </button>
                )}
                {managedByPanel && (
                  <button
                    className="svc-cfg-btn danger"
                    onClick={() => { setRemoveFor(site); setRemoveInput('') }}
                    disabled={busy}
                  >
                    {t('ssl.removeCert')}
                  </button>
                )}
              </div>
            </div>
          )
        })}
      </div>

      {activeLog && !uploadFor && !selfSignedFor && (
        <div className="ssl-log-section">
          <div className="ssl-log-header">
            <span>{activeLog.domain}</span>
            {activeLog.status === 'installing' && <div className="install-spinner" />}
            {activeLog.status === 'done' && <span style={{ color: 'var(--green)' }}>Done</span>}
            {activeLog.status === 'error' && <span style={{ color: 'var(--red)' }}>Error</span>}
          </div>
          <div className="install-log">
            {activeLog.lines.map((line, i) => (
              <div
                className={`install-log-line${activeLog.status === 'error' && i === activeLog.lines.length - 1 ? ' error' : ''}`}
                key={i}
              >
                {line}
              </div>
            ))}
            <div ref={logEndRef} />
          </div>
        </div>
      )}

      <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.8 }}>
        <div>{t('ssl.infoNotice')} <a href="#" onClick={(e) => { e.preventDefault(); openUrl('https://letsencrypt.org/') }} style={{ color: 'var(--accent)' }}>https://letsencrypt.org/</a></div>
        <div style={{ color: 'var(--yellow)', fontWeight: 600 }}>{t('ssl.infoWarning')}</div>
      </div>

      {uploadFor && sessionId && (
        <UploadCertModal
          sessionId={sessionId}
          site={uploadFor}
          log={logs[uploadFor.domain]}
          onClose={() => setUploadFor(null)}
          onDone={async () => {
            setUploadFor(null)
            await emit('ssl-installed', { sessionId, domain: uploadFor.domain })
            fetchAll()
          }}
        />
      )}

      {selfSignedFor && sessionId && (
        <SelfSignedModal
          sessionId={sessionId}
          site={selfSignedFor}
          log={logs[selfSignedFor.domain]}
          onClose={() => setSelfSignedFor(null)}
          onDone={async () => {
            setSelfSignedFor(null)
            await emit('ssl-installed', { sessionId, domain: selfSignedFor.domain })
            fetchAll()
          }}
        />
      )}

      {detail && (
        <CertDetailModal info={detail} onClose={() => setDetail(null)} />
      )}

      {removeFor && sessionId && (
        <RemoveCertModal
          sessionId={sessionId}
          site={removeFor}
          input={removeInput}
          setInput={setRemoveInput}
          log={logs[removeFor.domain]}
          onClose={() => setRemoveFor(null)}
          onDone={async () => {
            const domain = removeFor.domain
            setRemoveFor(null)
            await emit('ssl-installed', { sessionId, domain })
            fetchAll()
          }}
        />
      )}
    </div>
  )
}

// ===== 上传自定义证书 =====

function UploadCertModal({
  sessionId, site, log, onClose, onDone,
}: {
  sessionId: string
  site: SiteInfo
  log?: SslLog
  onClose: () => void
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [certPem, setCertPem] = useState('')
  const [keyPem, setKeyPem] = useState('')
  const [chainPem, setChainPem] = useState('')
  const [stopLe, setStopLe] = useState(false)
  const [checking, setChecking] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [precheck, setPrecheck] = useState<SslPrecheck | null>(null)
  const [error, setError] = useState('')

  const siteDomains = (site.domains || site.domain).split(/\s+/).filter(Boolean)

  const pick = async (setter: (v: string) => void) => {
    try {
      const path = await openFile({ multiple: false, filters: PEM_FILTERS })
      if (typeof path === 'string') {
        const text = await readTextFile(path)
        setter(text)
        setPrecheck(null)
      }
    } catch (e) {
      setError(`${t('ssl.fileReadFailed')}: ${String(e)}`)
    }
  }

  const doCheck = async () => {
    if (!certPem.trim() || !keyPem.trim()) {
      setError(t('ssl.certRequired'))
      return
    }
    setError('')
    setChecking(true)
    setPrecheck(null)
    try {
      const res = await invoke<SslPrecheck>('server_ssl_precheck', {
        sessionId,
        domain: site.domain,
        certPem,
        keyPem,
        chainPem: chainPem.trim() || null,
        siteDomains,
      })
      setPrecheck(res)
    } catch (e) {
      setError(String(e))
    } finally {
      setChecking(false)
    }
  }

  const doInstall = async () => {
    setError('')
    setInstalling(true)
    try {
      await invoke<string>('server_ssl_install_custom', {
        sessionId,
        domain: site.domain,
        configPath: site.config_path,
        certPem,
        keyPem,
        chainPem: chainPem.trim() || null,
        stopLetsencrypt: stopLe,
        siteDomains,
      })
      onDone()
    } catch (e) {
      setError(String(e))
    } finally {
      setInstalling(false)
    }
  }

  const vars = (it: SslCheckItem) => ({
    count: it.detail,
    notAfter: it.detail,
    domains: it.detail,
    type: it.detail,
    days: it.days,
  })

  return (
    <div className="sidebar-confirm-overlay" onClick={onClose}>
      <div className="ssl-modal" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('ssl.uploadTitle', { domain: site.domain })}</div>
          <button className="sidebar-edit-close" onClick={onClose}>×</button>
        </div>

        <div className="ssl-modal-body">
          <div className="form-group">
            <label>{t('ssl.certPemLabel')}</label>
            <textarea
              className="ssl-textarea"
              value={certPem}
              onChange={(e) => { setCertPem(e.target.value); setPrecheck(null) }}
              placeholder={t('ssl.certPemPlaceholder')}
              rows={5}
              spellCheck={false}
            />
            <button className="svc-cfg-btn ssl-file-btn" onClick={() => pick(setCertPem)}>
              {t('ssl.chooseFile')}
            </button>
          </div>

          <div className="form-group">
            <label>{t('ssl.keyPemLabel')}</label>
            <textarea
              className="ssl-textarea"
              value={keyPem}
              onChange={(e) => { setKeyPem(e.target.value); setPrecheck(null) }}
              placeholder={t('ssl.keyPemPlaceholder')}
              rows={5}
              spellCheck={false}
            />
            <button className="svc-cfg-btn ssl-file-btn" onClick={() => pick(setKeyPem)}>
              {t('ssl.chooseFile')}
            </button>
          </div>

          <div className="form-group">
            <label>{t('ssl.chainPemLabel')}</label>
            <textarea
              className="ssl-textarea"
              value={chainPem}
              onChange={(e) => { setChainPem(e.target.value); setPrecheck(null) }}
              placeholder={t('ssl.chainPemPlaceholder')}
              rows={3}
              spellCheck={false}
            />
          </div>

          <label className="ssl-checkbox-row">
            <input type="checkbox" checked={stopLe} onChange={(e) => setStopLe(e.target.checked)} />
            <span>
              {t('ssl.stopLetsEncrypt')}
              <span className="ssl-hint">{t('ssl.stopLetsEncryptHint')}</span>
            </span>
          </label>

          {precheck && (
            <div className="ssl-check-list">
              {precheck.items.map((it, i) => (
                <div key={i} className={`ssl-check-item ${it.level}`}>
                  <span className="ssl-check-mark">{it.level === 'ok' ? '✓' : it.level === 'warn' ? '!' : '×'}</span>
                  <span>{t(`ssl.check.${it.code}`, vars(it))}</span>
                </div>
              ))}
              <div className={`ssl-check-verdict ${precheck.ok ? 'ok' : 'error'}`}>
                {precheck.ok ? t('ssl.precheckOk') : t('ssl.precheckFailed')}
              </div>
            </div>
          )}

          {log && log.lines.length > 0 && (
            <div className="install-log ssl-modal-log">
              {log.lines.slice(-8).map((line, i) => (
                <div className={`install-log-line${log.status === 'error' && i === log.lines.slice(-8).length - 1 ? ' error' : ''}`} key={i}>
                  {line}
                </div>
              ))}
            </div>
          )}

          {error && <div className="ssl-error">{error}</div>}
        </div>

        <div className="sidebar-confirm-actions">
          <button className="sidebar-confirm-btn" onClick={onClose}>{t('common.cancel')}</button>
          <button className="sidebar-confirm-btn" onClick={doCheck} disabled={checking || installing}>
            {checking ? t('ssl.checking') : t('ssl.precheck')}
          </button>
          <button
            className="sidebar-confirm-btn primary"
            onClick={doInstall}
            disabled={installing || (precheck !== null && !precheck.ok)}
          >
            {installing ? t('ssl.installRunning') : t('ssl.installCert')}
          </button>
        </div>
      </div>
    </div>
  )
}

// ===== 自签证书 =====

function SelfSignedModal({
  sessionId, site, log, onClose, onDone,
}: {
  sessionId: string
  site: SiteInfo
  log?: SslLog
  onClose: () => void
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [days, setDays] = useState(365)
  const [installing, setInstalling] = useState(false)
  const [error, setError] = useState('')
  const siteDomains = (site.domains || site.domain).split(/\s+/).filter(Boolean)

  const doInstall = async () => {
    setError('')
    setInstalling(true)
    try {
      await invoke<string>('server_ssl_generate_self_signed', {
        sessionId,
        domain: site.domain,
        configPath: site.config_path,
        days,
        siteDomains,
      })
      onDone()
    } catch (e) {
      setError(String(e))
    } finally {
      setInstalling(false)
    }
  }

  return (
    <div className="sidebar-confirm-overlay" onClick={onClose}>
      <div className="ssl-modal" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('ssl.selfSignedTitle', { domain: site.domain })}</div>
          <button className="sidebar-edit-close" onClick={onClose}>×</button>
        </div>
        <div className="ssl-modal-body">
          <div className="form-group">
            <label>{t('ssl.selfSignedDays')}</label>
            <input
              className="sidebar-edit-input"
              type="number"
              min={1}
              max={3650}
              value={days}
              onChange={(e) => setDays(Number(e.target.value) || 0)}
            />
          </div>
          <div className="ssl-hint">{t('ssl.selfSignedHint')}</div>
          {log && log.lines.length > 0 && (
            <div className="install-log ssl-modal-log">
              {log.lines.slice(-8).map((line, i) => (
                <div className="install-log-line" key={i}>{line}</div>
              ))}
            </div>
          )}
          {error && <div className="ssl-error">{error}</div>}
        </div>
        <div className="sidebar-confirm-actions">
          <button className="sidebar-confirm-btn" onClick={onClose}>{t('common.cancel')}</button>
          <button
            className="sidebar-confirm-btn primary"
            onClick={doInstall}
            disabled={installing || days <= 0 || days > 3650}
          >
            {installing ? t('ssl.installRunning') : t('ssl.selfSignedGenerate')}
          </button>
        </div>
      </div>
    </div>
  )
}

// ===== 证书详情 =====

function CertDetailModal({ info, onClose }: { info: SslCertInfo; onClose: () => void }) {
  const { t } = useTranslation()
  const rows: Array<[string, string]> = [
    [t('ssl.certSubject'), info.subject || '-'],
    [t('ssl.certIssuer'), info.issuer || '-'],
    [t('ssl.certValidity'), `${info.not_before || '-'} → ${info.not_after || '-'}`],
    [t('ssl.certSans'), (info.sans || []).join(', ') || '-'],
    [t('ssl.certKey'), info.key_info || '-'],
    [t('ssl.certChain'), t('ssl.certChainValue', { count: info.chain_length })],
    [t('ssl.certFingerprint'), info.fingerprint || '-'],
    [t('ssl.certPathLabel'), info.cert_path || '-'],
    [t('ssl.keyPathLabel'), info.key_path || '-'],
  ]

  return (
    <div className="sidebar-confirm-overlay" onClick={onClose}>
      <div className="ssl-modal" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('ssl.certDetailTitle', { domain: info.domain })}</div>
          <button className="sidebar-edit-close" onClick={onClose}>×</button>
        </div>
        <div className="ssl-modal-body">
          {rows.map(([label, value], i) => (
            <div key={i} className="ssl-detail-row">
              <span className="ssl-detail-label">{label}</span>
              <span className="ssl-detail-value">{value}</span>
            </div>
          ))}
        </div>
        <div className="sidebar-confirm-actions">
          <button className="sidebar-confirm-btn primary" onClick={onClose}>{t('ssl.close')}</button>
        </div>
      </div>
    </div>
  )
}

// ===== 移除证书 =====

function RemoveCertModal({
  sessionId, site, input, setInput, log, onClose, onDone,
}: {
  sessionId: string
  site: SiteInfo
  input: string
  setInput: (v: string) => void
  log?: SslLog
  onClose: () => void
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const confirmed = input.trim() === site.domain

  const doRemove = async () => {
    if (!confirmed) return
    setError('')
    setBusy(true)
    try {
      await invoke<string>('server_ssl_remove_custom', {
        sessionId,
        domain: site.domain,
        configPath: site.config_path,
      })
      onDone()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="sidebar-confirm-overlay" onClick={onClose}>
      <div className="ssl-modal" onClick={(e) => e.stopPropagation()}>
        <div className="sidebar-edit-header">
          <div className="sidebar-confirm-title">{t('ssl.removeTitle', { domain: site.domain })}</div>
          <button className="sidebar-edit-close" onClick={onClose}>×</button>
        </div>
        <div className="ssl-modal-body">
          <div className="ssl-warning-text">{t('ssl.removeWarning')}</div>
          <div className="form-group">
            <label>{t('ssl.removeConfirmLabel', { domain: site.domain })}</label>
            <input
              className="sidebar-edit-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={site.domain}
            />
          </div>
          {log && log.lines.length > 0 && (
            <div className="install-log ssl-modal-log">
              {log.lines.slice(-8).map((line, i) => (
                <div className="install-log-line" key={i}>{line}</div>
              ))}
            </div>
          )}
          {error && <div className="ssl-error">{error}</div>}
        </div>
        <div className="sidebar-confirm-actions">
          <button className="sidebar-confirm-btn" onClick={onClose}>{t('common.cancel')}</button>
          <button className="sidebar-confirm-btn danger" onClick={doRemove} disabled={!confirmed || busy}>
            {busy ? t('ssl.installRunning') : t('ssl.removeAction')}
          </button>
        </div>
      </div>
    </div>
  )
}
