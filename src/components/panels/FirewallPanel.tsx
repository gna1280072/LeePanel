import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface FirewallRule {
  id: string
  port: string
  protocol: string
  action: string
  source: string
  raw: string
}

interface FirewallInfo {
  firewall_type: string
  enabled: boolean
  rules: FirewallRule[]
}

interface FirewallPanelProps {
  sessionId: string | null
}

export default function FirewallPanel({ sessionId }: FirewallPanelProps) {
  const [info, setInfo] = useState<FirewallInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [actionLoading, setActionLoading] = useState('')

  // Add rule form
  const [showAdd, setShowAdd] = useState(false)
  const [newPort, setNewPort] = useState('')
  const [newProtocol, setNewProtocol] = useState('tcp')
  const [newAction, setNewAction] = useState('allow')

  // Confirm delete
  const [confirmDelete, setConfirmDelete] = useState<FirewallRule | null>(null)
  const [toggling, setToggling] = useState(false)

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

  const handleAdd = async () => {
    if (!sessionId || !newPort.trim()) return
    setActionLoading('add')
    try {
      await invoke('server_firewall_add', {
        sessionId,
        port: newPort.trim(),
        protocol: newProtocol,
        action: newAction,
      })
      setNewPort('')
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
    try {
      await invoke('server_firewall_remove', {
        sessionId,
        port: rule.port,
        protocol: rule.protocol,
        action: rule.action,
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
    try {
      await invoke('server_firewall_toggle', { sessionId, enable })
      await fetchRules()
    } catch (e) {
      setError(String(e))
    } finally {
      setToggling(false)
    }
  }

  if (!sessionId) return <div className="sp-empty">Connect to a server first</div>

  return (
    <div className="firewall-panel">
      <div className="firewall-header">
        <h2>Firewall</h2>
        <button className="firewall-refresh" onClick={fetchRules} disabled={loading}>
          {loading ? '...' : '↻ Refresh'}
        </button>
      </div>

      {error && <div className="firewall-error">{error}</div>}

      {loading && !info && <div className="sp-loading">Detecting firewall...</div>}

      {info && (
        <>
          {/* Firewall Status */}
          <div className="firewall-status">
            <span className={`firewall-badge ${info.firewall_type === 'none' ? 'none' : info.enabled ? 'active' : 'inactive'}`}>
              {info.firewall_type === 'none'
                ? 'No Firewall Detected'
                : `${info.firewall_type.toUpperCase()} — ${info.enabled ? 'Active' : 'Inactive'}`}
            </span>
            <span className="firewall-rule-count">{info.rules.length} rules</span>
            {info.firewall_type !== 'none' && (
              <button
                className={`firewall-toggle ${info.enabled ? 'on' : 'off'} ${toggling ? 'loading' : ''}`}
                onClick={handleToggle}
                disabled={toggling}
                title={info.enabled ? 'Disable Firewall' : 'Enable Firewall'}
              >
                <span className="toggle-track">
                  <span className="toggle-thumb" />
                </span>
                <span className="toggle-label">{info.enabled ? 'ON' : 'OFF'}</span>
              </button>
            )}
          </div>

          {/* Add Rule Button */}
          {info.firewall_type !== 'none' && (
            <div className={`firewall-actions ${!info.enabled ? 'disabled' : ''}`}>
              <button
                className="firewall-add-btn"
                onClick={() => setShowAdd(!showAdd)}
                disabled={!info.enabled}
              >
                {showAdd ? '✕ Cancel' : '+ Add Rule'}
              </button>
            </div>
          )}

          {/* Add Rule Form */}
          {showAdd && info.enabled && (
            <div className="firewall-add-form">
              <div className="firewall-form-row">
                <div className="firewall-form-group">
                  <label>Port</label>
                  <input
                    value={newPort}
                    onChange={(e) => setNewPort(e.target.value)}
                    placeholder="80, 8080-8090"
                    onKeyDown={(e) => { if (e.key === 'Enter') handleAdd() }}
                  />
                </div>
                <div className="firewall-form-group" style={{ width: 90 }}>
                  <label>Protocol</label>
                  <select value={newProtocol} onChange={(e) => setNewProtocol(e.target.value)}>
                    <option value="tcp">TCP</option>
                    <option value="udp">UDP</option>
                    <option value="both">Both</option>
                  </select>
                </div>
                <div className="firewall-form-group" style={{ width: 90 }}>
                  <label>Action</label>
                  <select value={newAction} onChange={(e) => setNewAction(e.target.value)}>
                    <option value="allow">Allow</option>
                    <option value="deny">Deny</option>
                    <option value="reject">Reject</option>
                  </select>
                </div>
                <div className="firewall-form-group" style={{ alignSelf: 'flex-end' }}>
                  <button
                    className="firewall-submit-btn"
                    onClick={handleAdd}
                    disabled={actionLoading === 'add' || !newPort.trim()}
                  >
                    {actionLoading === 'add' ? '...' : 'Add'}
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Rules Table */}
          {info.rules.length > 0 ? (
            <div className={`firewall-rules-table ${!info.enabled ? 'disabled' : ''}`}>
              <div className="firewall-table-header">
                <span className="fw-col-port">Port</span>
                <span className="fw-col-proto">Protocol</span>
                <span className="fw-col-action">Action</span>
                <span className="fw-col-source">Source</span>
                <span className="fw-col-ops"></span>
              </div>
              {info.rules.map((rule) => (
                <div className="firewall-table-row" key={rule.id}>
                  <span className="fw-col-port">{rule.port}</span>
                  <span className="fw-col-proto">{rule.protocol.toUpperCase()}</span>
                  <span className={`fw-col-action fw-action-${rule.action}`}>{rule.action.toUpperCase()}</span>
                  <span className="fw-col-source">{rule.source}</span>
                  <span className="fw-col-ops">
                    <button
                      className="fw-delete-btn"
                      onClick={() => setConfirmDelete(rule)}
                      disabled={!!actionLoading || !info.enabled}
                      title="Remove rule"
                    >
                      ✕
                    </button>
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <div className={`firewall-empty ${!info.enabled ? 'disabled' : ''}`}>
              {info.firewall_type === 'none'
                ? 'No supported firewall (ufw, firewalld, iptables) found on this server.'
                : info.enabled
                  ? 'No firewall rules configured.'
                  : 'Firewall is disabled. Turn it on to manage rules.'}
            </div>
          )}
        </>
      )}

      {/* Confirm Delete Dialog */}
      {confirmDelete && (
        <div className="firewall-confirm-overlay" onClick={() => setConfirmDelete(null)}>
          <div className="firewall-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="firewall-confirm-title">Remove Rule</div>
            <div className="firewall-confirm-msg">
              Remove rule: <strong>{confirmDelete.port}/{confirmDelete.protocol}</strong> ({confirmDelete.action})?
            </div>
            <div className="firewall-confirm-actions">
              <button className="firewall-confirm-btn cancel" onClick={() => setConfirmDelete(null)}>Cancel</button>
              <button
                className="firewall-confirm-btn danger"
                onClick={() => handleRemove(confirmDelete)}
                disabled={!!actionLoading}
              >
                {actionLoading === confirmDelete.id ? '...' : 'Remove'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
