import { useState, useEffect, useCallback, useMemo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface FirewallRule {
  id: string
  port: string
  protocol: string
  action: string
  source: string
  raw: string
}

interface FirewallToggleResult {
  enabled: boolean
  ssh_port_auto_opened: boolean
  ssh_port: number
}

interface FirewallInfo {
  firewall_type: string
  enabled: boolean
  rules: FirewallRule[]
}

interface FirewallPanelProps {
  sessionId: string | null
}

// Common service ports for one-click open/close
const QUICK_PORTS = [
  { port: '80', protocol: 'tcp', label: 'HTTP' },
  { port: '443', protocol: 'tcp', label: 'HTTPS' },
  { port: '3306', protocol: 'tcp', label: 'MySQL' },
  { port: '5432', protocol: 'tcp', label: 'PostgreSQL' },
  { port: '6379', protocol: 'tcp', label: 'Redis' },
  { port: '27017', protocol: 'tcp', label: 'MongoDB' },
  { port: '21', protocol: 'tcp', label: 'FTP' },
]

export default function FirewallPanel({ sessionId }: FirewallPanelProps) {
  const { t } = useTranslation()
  const [info, setInfo] = useState<FirewallInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [actionLoading, setActionLoading] = useState('')
  const [notice, setNotice] = useState<{ type: 'success' | 'info', text: string } | null>(null)

  // Add rule form
  const [showAdd, setShowAdd] = useState(false)
  const [newPort, setNewPort] = useState('')
  const [newProtocol, setNewProtocol] = useState('tcp')
  const [newAction, setNewAction] = useState('allow')
  const [newSource, setNewSource] = useState('')

  // Filters / interactions
  const [search, setSearch] = useState('')
  const [actionFilter, setActionFilter] = useState('all')
  const [expandedId, setExpandedId] = useState<string | null>(null)

  // Confirm dialogs
  const [confirmDelete, setConfirmDelete] = useState<FirewallRule | null>(null)
  const [toggling, setToggling] = useState(false)
  const [sshPortNotice, setSshPortNotice] = useState<string | null>(null)

  const fetchRules = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const result = await invoke<FirewallInfo>('server_firewall_list', { sessionId })
      setInfo(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => {
    fetchRules()
  }, [fetchRules])

  // ---- statistics ----
  const stats = useMemo(() => {
    const rules = info?.rules ?? []
    return {
      total: rules.length,
      allow: rules.filter(r => r.action === 'allow').length,
      deny: rules.filter(r => r.action === 'deny').length,
      reject: rules.filter(r => r.action === 'reject').length,
    }
  }, [info])

  // ---- filtered rules ----
  const filteredRules = useMemo(() => {
    let rules = info?.rules ?? []
    if (actionFilter !== 'all') rules = rules.filter(r => r.action === actionFilter)
    if (search.trim()) {
      const q = search.trim().toLowerCase()
      rules = rules.filter(r =>
        r.port.toLowerCase().includes(q) ||
        r.protocol.toLowerCase().includes(q) ||
        r.source.toLowerCase().includes(q) ||
        r.raw.toLowerCase().includes(q)
      )
    }
    return rules
  }, [info, search, actionFilter])

  const isPortOpen = useCallback((port: string, protocol: string) =>
    (info?.rules ?? []).some(r =>
      r.action === 'allow' &&
      r.port === port &&
      (r.protocol === protocol || r.protocol === 'any' || r.protocol === 'both')
    ), [info])

  // ---- actions ----
  const flashNotice = (type: 'success' | 'info', text: string) => {
    setNotice({ type, text })
    setTimeout(() => setNotice(null), 3000)
  }

  const handleAdd = async () => {
    if (!sessionId || !newPort.trim()) return
    setActionLoading('add')
    setError('')
    try {
      await invoke('server_firewall_add', {
        sessionId,
        port: newPort.trim(),
        protocol: newProtocol,
        action: newAction,
        source: newSource.trim() || null,
      })
      setNewPort('')
      setNewSource('')
      setShowAdd(false)
      await fetchRules()
    } catch (e) {
      setError(String(e))
    } finally {
      setActionLoading('')
    }
  }

  const handleRemove = async (rule: FirewallRule) => {
    if (!sessionId) return
    setActionLoading(rule.id)
    setError('')
    try {
      await invoke('server_firewall_remove', {
        sessionId,
        port: rule.port,
        protocol: rule.protocol,
        action: rule.action,
        source: rule.source === 'Anywhere' || rule.source === 'anywhere' ? null : rule.source,
      })
      setConfirmDelete(null)
      await fetchRules()
    } catch (e) {
      setError(String(e))
    } finally {
      setActionLoading('')
    }
  }

  const handleToggle = async () => {
    if (!sessionId || !info || info.firewall_type === 'none') return
    const enable = !info.enabled
    setToggling(true)
    setError('')
    setSshPortNotice(null)
    try {
      const result = await invoke<FirewallToggleResult>('server_firewall_toggle', { sessionId, enable })
      if (result.ssh_port_auto_opened) {
        setSshPortNotice(t('firewall.sshPortAutoOpened', { port: result.ssh_port }))
      }
      await fetchRules()
    } catch (e) {
      setError(String(e))
    } finally {
      setToggling(false)
    }
  }

  const handleQuickToggle = async (qp: { port: string, protocol: string }) => {
    if (!sessionId || !info || !info.enabled) return
    const open = isPortOpen(qp.port, qp.protocol)
    setActionLoading('quick-' + qp.port)
    setError('')
    try {
      if (open) {
        await invoke('server_firewall_remove', {
          sessionId,
          port: qp.port,
          protocol: qp.protocol,
          action: 'allow',
          source: null,
        })
        flashNotice('info', t('firewall.portClosed', { port: qp.port }))
      } else {
        await invoke('server_firewall_add', {
          sessionId,
          port: qp.port,
          protocol: qp.protocol,
          action: 'allow',
          source: null,
        })
        flashNotice('success', t('firewall.portOpened', { port: qp.port }))
      }
      await fetchRules()
    } catch (e) {
      setError(String(e))
    } finally {
      setActionLoading('')
    }
  }

  const copyText = async (text: string, msgKey: string) => {
    try {
      await navigator.clipboard.writeText(text)
      flashNotice('success', t(msgKey))
    } catch (e) {
      setError(String(e))
    }
  }

  const handleCopyAll = () => {
    if (!info) return
    const header = `# ${info.firewall_type.toUpperCase()} rules (${stats.total})`
    const text = [header, ...info.rules.map(r => r.raw)].join('\n')
    copyText(text, 'firewall.copiedAll')
  }

  const handleCopyRule = (rule: FirewallRule) => {
    copyText(rule.raw, 'firewall.copied')
  }

  if (!sessionId) return <div className="sp-empty">{t('common.connectFirst')}</div>

  const managed = info && info.firewall_type !== 'none'

  return (
    <div className="firewall-panel">
      <div className="firewall-header">
        <h2>{t('firewall.title')}</h2>
        <div className="firewall-header-actions">
          {managed && stats.total > 0 && (
            <button className="firewall-refresh" onClick={handleCopyAll} disabled={!!actionLoading}>
              ⧉ {t('firewall.copyAll')}
            </button>
          )}
          <button className="firewall-refresh" onClick={fetchRules} disabled={loading}>
            {loading ? '...' : `↻ ${t('common.refresh')}`}
          </button>
        </div>
      </div>

      {error && <div className="firewall-error">{error}</div>}

      {notice && (
        <div className={`firewall-notice ${notice.type}`}>
          <span>{notice.type === 'success' ? '✅ ' : 'ℹ️ '}{notice.text}</span>
          <button className="firewall-notice-close" onClick={() => setNotice(null)}>✕</button>
        </div>
      )}

      {sshPortNotice && (
        <div className="firewall-notice" onClick={() => setSshPortNotice(null)}>
          <span>🛡️ {sshPortNotice}</span>
          <button className="firewall-notice-close" onClick={(e) => { e.stopPropagation(); setSshPortNotice(null) }}>✕</button>
        </div>
      )}

      {loading && !info && <div className="sp-loading">{t('firewall.detecting')}</div>}

      {info && (
        <>
          {/* Status bar */}
          <div className="firewall-status">
            <span className={`firewall-badge ${info.firewall_type === 'none' ? 'none' : info.enabled ? 'active' : 'inactive'}`}>
              {info.firewall_type === 'none'
                ? t('firewall.noFirewall')
                : `${info.firewall_type.toUpperCase()} — ${info.enabled ? t('firewall.active') : t('firewall.inactive')}`}
            </span>
            {managed && (
              <button
                className={`firewall-toggle ${info.enabled ? 'on' : 'off'} ${toggling ? 'loading' : ''}`}
                onClick={handleToggle}
                disabled={toggling}
                title={info.enabled ? t('firewall.disableFirewall') : t('firewall.enableFirewall')}
              >
                <span className="toggle-track">
                  <span className="toggle-thumb" />
                </span>
                <span className="toggle-label">{info.enabled ? t('common.on') : t('common.off')}</span>
              </button>
            )}
          </div>

          {/* Stats cards */}
          <div className="fw-stats">
            <div className="fw-stat-card total">
              <span className="fw-stat-num">{stats.total}</span>
              <span className="fw-stat-label">{t('firewall.statsTotal')}</span>
            </div>
            <div className="fw-stat-card allow">
              <span className="fw-stat-num">{stats.allow}</span>
              <span className="fw-stat-label">{t('firewall.allow')}</span>
            </div>
            <div className="fw-stat-card deny">
              <span className="fw-stat-num">{stats.deny}</span>
              <span className="fw-stat-label">{t('firewall.deny')}</span>
            </div>
            <div className="fw-stat-card reject">
              <span className="fw-stat-num">{stats.reject}</span>
              <span className="fw-stat-label">{t('firewall.reject')}</span>
            </div>
          </div>

          {/* Quick ports */}
          {managed && info.enabled && (
            <div className="fw-quick-ports">
              <div className="fw-section-title">
                {t('firewall.quickTitle')}
                <span className="fw-section-hint">{t('firewall.quickHint')}</span>
              </div>
              <div className="fw-quick-grid">
                {QUICK_PORTS.map(qp => {
                  const open = isPortOpen(qp.port, qp.protocol)
                  const busy = actionLoading === 'quick-' + qp.port
                  return (
                    <button
                      key={qp.port}
                      className={`fw-quick-chip ${open ? 'open' : ''}`}
                      onClick={() => handleQuickToggle(qp)}
                      disabled={!!actionLoading}
                      title={open ? t('firewall.portOpenTitle', { port: qp.port }) : t('firewall.portClosedTitle', { port: qp.port })}
                    >
                      <span className="fw-quick-label">{qp.label}</span>
                      <span className="fw-quick-port">{qp.port}</span>
                      <span className={`fw-quick-dot ${open ? 'open' : ''}`}>{busy ? '…' : open ? '✓' : '+'}</span>
                    </button>
                  )
                })}
              </div>
            </div>
          )}

          {/* Toolbar */}
          <div className="fw-toolbar">
            <input
              className="fw-search"
              placeholder={t('firewall.searchPlaceholder')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            <select className="fw-filter" value={actionFilter} onChange={(e) => setActionFilter(e.target.value)}>
              <option value="all">{t('firewall.allActions')}</option>
              <option value="allow">{t('firewall.allow')}</option>
              <option value="deny">{t('firewall.deny')}</option>
              <option value="reject">{t('firewall.reject')}</option>
            </select>
            <div className="fw-toolbar-spacer" />
            {managed && info.enabled && (
              <>
                <button
                  className="firewall-add-btn"
                  onClick={() => setShowAdd(!showAdd)}
                >
                  {showAdd ? `✕ ${t('common.cancel')}` : t('firewall.addRule')}
                </button>
              </>
            )}
          </div>

          {/* Add Rule Form */}
          {showAdd && managed && info.enabled && (
            <div className="firewall-add-form">
              <div className="firewall-form-row">
                <div className="firewall-form-group">
                  <label>{t('firewall.port')}</label>
                  <input
                    value={newPort}
                    onChange={(e) => setNewPort(e.target.value)}
                    placeholder="80, 8080-8090"
                    onKeyDown={(e) => { if (e.key === 'Enter') handleAdd() }}
                  />
                </div>
                <div className="firewall-form-group" style={{ width: 90 }}>
                  <label>{t('firewall.protocol')}</label>
                  <select value={newProtocol} onChange={(e) => setNewProtocol(e.target.value)}>
                    <option value="tcp">TCP</option>
                    <option value="udp">UDP</option>
                    <option value="both">Both</option>
                  </select>
                </div>
                <div className="firewall-form-group" style={{ width: 90 }}>
                  <label>{t('firewall.action')}</label>
                  <select value={newAction} onChange={(e) => setNewAction(e.target.value)}>
                    <option value="allow">{t('firewall.allow')}</option>
                    <option value="deny">{t('firewall.deny')}</option>
                    <option value="reject">{t('firewall.reject')}</option>
                  </select>
                </div>
                <div className="firewall-form-group" style={{ flex: 1, minWidth: 160 }}>
                  <label>{t('firewall.sourceLabel')}</label>
                  <input
                    value={newSource}
                    onChange={(e) => setNewSource(e.target.value)}
                    placeholder={t('firewall.sourcePlaceholder')}
                  />
                </div>
                <div className="firewall-form-group" style={{ alignSelf: 'flex-end' }}>
                  <button
                    className="firewall-submit-btn"
                    onClick={handleAdd}
                    disabled={actionLoading === 'add' || !newPort.trim()}
                  >
                    {actionLoading === 'add' ? '...' : t('common.create')}
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Rules Table */}
          {stats.total > 0 ? (
            filteredRules.length > 0 ? (
              <div className={`firewall-rules-table ${!info.enabled ? 'disabled' : ''}`}>
                <div className="firewall-table-header">
                  <span className="fw-col-port">{t('firewall.port')}</span>
                  <span className="fw-col-proto">{t('firewall.protocol')}</span>
                  <span className="fw-col-action">{t('firewall.action')}</span>
                  <span className="fw-col-source">{t('firewall.source')}</span>
                  <span className="fw-col-ops"></span>
                </div>
                {filteredRules.map((rule) => (
                  <div key={rule.id}>
                    <div
                      className={`firewall-table-row ${expandedId === rule.id ? 'expanded' : ''}`}
                      onClick={() => setExpandedId(expandedId === rule.id ? null : rule.id)}
                    >
                      <span className="fw-col-port">{rule.port}</span>
                      <span className="fw-col-proto">{rule.protocol.toUpperCase()}</span>
                      <span className={`fw-col-action fw-action-${rule.action}`}>{rule.action.toUpperCase()}</span>
                      <span className="fw-col-source fw-col-source-text" title={rule.source}>
                        {rule.source}
                      </span>
                      <span className="fw-col-ops" onClick={(e) => e.stopPropagation()}>
                        <button
                          className="fw-copy-btn"
                          onClick={() => handleCopyRule(rule)}
                          disabled={!!actionLoading}
                          title={t('firewall.copyRule')}
                        >
                          ⧉
                        </button>
                        <button
                          className="fw-delete-btn"
                          onClick={() => setConfirmDelete(rule)}
                          disabled={!!actionLoading || !info.enabled}
                          title={t('firewall.removeRule')}
                        >
                          ✕
                        </button>
                      </span>
                    </div>
                    {expandedId === rule.id && (
                      <div className="fw-rule-detail">
                        <div className="fw-rule-detail-head">
                          <span>{t('firewall.viewRaw')}</span>
                          <button className="firewall-refresh" onClick={() => handleCopyRule(rule)}>⧉ {t('firewall.copyRule')}</button>
                        </div>
                        <code className="fw-rule-raw">{rule.raw}</code>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className="firewall-empty">{t('firewall.noMatch')}</div>
            )
          ) : (
            <div className={`firewall-empty ${!info.enabled ? 'disabled' : ''}`}>
              {info.firewall_type === 'none'
                ? t('firewall.noFirewallFound')
                : info.enabled
                  ? t('firewall.noRules')
                  : t('firewall.disabledHint')}
            </div>
          )}
        </>
      )}

      {/* Confirm Delete Dialog */}
      {confirmDelete && (
        <div className="firewall-confirm-overlay" onClick={() => setConfirmDelete(null)}>
          <div className="firewall-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="firewall-confirm-title">{t('firewall.removeRule')}</div>
            <div className="firewall-confirm-msg">
              {t('firewall.removeRuleMsg', { port: confirmDelete.port, protocol: confirmDelete.protocol, action: confirmDelete.action })}
              {confirmDelete.source !== 'Anywhere' && confirmDelete.source !== 'anywhere' && (
                <div className="firewall-confirm-sub">From: {confirmDelete.source}</div>
              )}
            </div>
            <div className="firewall-confirm-actions">
              <button className="firewall-confirm-btn cancel" onClick={() => setConfirmDelete(null)}>{t('common.cancel')}</button>
              <button
                className="firewall-confirm-btn danger"
                onClick={() => handleRemove(confirmDelete)}
                disabled={!!actionLoading}
              >
                {actionLoading === confirmDelete.id ? '...' : t('firewall.remove')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
