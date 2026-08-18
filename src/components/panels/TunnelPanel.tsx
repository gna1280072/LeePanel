import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'

interface TunnelInfo {
  id: string
  session_id: string
  tunnel_type: string
  local_host: string
  local_port: number
  remote_host: string
  remote_port: number
  status: string
  created_at: number
  note: string
}

interface TunnelPanelProps {
  sessionId: string | null
  /** SSH 连接用的服务器主机名/IP（用于服务器转发 -R 的连接命令） */
  serverHost?: string
  /** SSH 用户名（用于 GatewayPorts 未开启时生成先 SSH 再连接的命令） */
  connUsername?: string
}

interface TunnelErrorPayload {
  tunnelId: string
  error: string
  code?: string
  target?: string
  sessionId?: string
}

interface TunnelStatusPayload {
  tunnelId: string
  status: string
  message?: string
}

interface GatewayPortsStatus {
  enabled: boolean
  value: string
}

type TunnelType = 'local' | 'remote' | 'dynamic'

export default function TunnelPanel({ sessionId, serverHost, connUsername: _connUsername }: TunnelPanelProps) {
  const { t } = useTranslation()
  const [tunnels, setTunnels] = useState<TunnelInfo[]>([])
  const [showCreate, setShowCreate] = useState(false)
  const [tunnelType, setTunnelType] = useState<TunnelType>('local')
  const [localHost, setLocalHost] = useState('127.0.0.1')
  const [localPort, setLocalPort] = useState('')
  const [remoteHost, setRemoteHost] = useState('127.0.0.1')
  const [remotePort, setRemotePort] = useState('')
  const [creating, setCreating] = useState(false)
  const [loading, setLoading] = useState(true)
  const [msg, setMsg] = useState('')
  const [error, setError] = useState('')
  const [gpStatus, setGpStatus] = useState<GatewayPortsStatus | null>(null)
  const [gpSaving, setGpSaving] = useState(false)
  const [showReconnectConfirm, setShowReconnectConfirm] = useState(false)
  const [reconnecting, setReconnecting] = useState(false)
  // 重连确认框对应的操作（开启/关闭），决定文案
  const [reconnectAction, setReconnectAction] = useState<'enable' | 'disable'>('enable')
  // 创建对话框内 GatewayPorts 栏的就地反馈（与顶部卡片消息相互独立）
  const [dlgGpMsg, setDlgGpMsg] = useState('')
  const [dlgGpMsgType, setDlgGpMsgType] = useState<'error' | 'success' | 'warn'>('error')
  const [restoringId, setRestoringId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<TunnelInfo | null>(null)
  const [deleteInput, setDeleteInput] = useState('')
  const [note, setNote] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editNote, setEditNote] = useState('')
  const [savingNote, setSavingNote] = useState(false)
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [deleteBatchTarget, setDeleteBatchTarget] = useState<TunnelInfo[] | null>(null)
  const [batchDeleteInput, setBatchDeleteInput] = useState('')
  const [deletingBatch, setDeletingBatch] = useState(false)
  const [restoringBatch, setRestoringBatch] = useState(false)
  const [closingBatch, setClosingBatch] = useState(false)

  const loadTunnels = useCallback(async () => {
    if (!sessionId) return
    try {
      setLoading(true)
      const result = await invoke<string>('tunnel_list', { sessionId })
      const allTunnels: TunnelInfo[] = JSON.parse(result)
      setTunnels(allTunnels)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  // 局部更新：只改指定行的字段，不触发全量拉取（无 loading 闪烁）
  const patchTunnel = useCallback((id: string, patch: Partial<TunnelInfo>) => {
    setTunnels(prev => prev.map(t => t.id === id ? { ...t, ...patch } : t))
  }, [])

  // 局部删除：从列表中移除指定行
  const removeTunnels = useCallback((ids: string[]) => {
    setTunnels(prev => prev.filter(t => !ids.includes(t.id)))
  }, [])

  const fetchGatewayPorts = useCallback(async () => {
    if (!sessionId) return
    try {
      const status = await invoke<GatewayPortsStatus>('server_get_gateway_ports_status', { sessionId })
      setGpStatus(status)
    } catch {
      setGpStatus(null)
    }
  }, [sessionId])

  // 创建对话框内「开启」按钮 / 「点击此处开启」链接 → 开启后弹重连确认框
  const handleEnableGatewayFromDialog = async () => {
    if (!sessionId) return
    setGpSaving(true)
    setDlgGpMsg('')
    try {
      await invoke<string>('server_set_gateway_ports', { sessionId, enable: true })
      await fetchGatewayPorts()
      setReconnectAction('enable')
      setShowReconnectConfirm(true)
    } catch (e) {
      setDlgGpMsgType('error')
      setDlgGpMsg(String(e))
    } finally {
      setGpSaving(false)
    }
  }

  // 创建对话框内「关闭」链接 → 关闭后弹重连确认框
  const handleDisableGatewayFromDialog = async () => {
    if (!sessionId) return
    setGpSaving(true)
    setDlgGpMsg('')
    try {
      await invoke<string>('server_set_gateway_ports', { sessionId, enable: false })
      await fetchGatewayPorts()
      setReconnectAction('disable')
      setShowReconnectConfirm(true)
    } catch (e) {
      setDlgGpMsgType('error')
      setDlgGpMsg(String(e))
    } finally {
      setGpSaving(false)
    }
  }

  // 同意 → 自动断开并重连（ssh_reconnect 内部先断开旧连接再重建，无需手动 ssh_disconnect）
  const handleReconnectAgree = async () => {
    if (!sessionId) return
    setReconnecting(true)
    try {
      await invoke('ssh_reconnect', { sessionId })
      setMsg(reconnectAction === 'disable' ? t('tunnel.reconnectDoneDisable') : t('tunnel.reconnectDone'))
      setShowReconnectConfirm(false)
      setShowCreate(false)
      // 重连后旧隧道已失效，刷新列表与 GatewayPorts 状态
      await Promise.all([loadTunnels(), fetchGatewayPorts()])
    } catch (e) {
      setDlgGpMsgType('error')
      setDlgGpMsg(t('tunnel.reconnectFailed', { error: String(e) }))
      setShowReconnectConfirm(false)
    } finally {
      setReconnecting(false)
    }
  }

  // 拒绝 → 引导手动重连
  const handleReconnectLater = () => {
    setShowReconnectConfirm(false)
    setDlgGpMsgType('warn')
    setDlgGpMsg(reconnectAction === 'disable' ? t('tunnel.reconnectManualHintDisable') : t('tunnel.reconnectManualHint'))
  }

  useEffect(() => {
    loadTunnels()
    fetchGatewayPorts()

    // 注意：不监听 tunnel-created —— 创建/启动走 create_tunnel 都会发它，
    // 而两者的列表状态已由 invoke 路径本地插入/patch；此处再全量刷新会导致
    // 点「启动」时整页闪烁。
    const unlistenStatus = listen<TunnelStatusPayload>('tunnel-status', (event) => {
      const { tunnelId, status } = event.payload
      patchTunnel(tunnelId, { status: status === 'listening' ? 'active' : 'stopped' })
    })
    const unlistenError = listen<TunnelErrorPayload>('tunnel-error', (event) => {
      const { code, target, error, sessionId: errSessionId } = event.payload
      // Ignore errors from tunnels belonging to other server connections
      if (errSessionId && errSessionId !== sessionId) return
      if (code === 'connect_failed' && target) {
        setError(t('tunnel.errors.connectFailed', { target }))
      } else if (code === 'prohibited' && target) {
        setError(t('tunnel.errors.prohibited', { target }))
      } else if (code === 'local_connect_failed' && target) {
        setError(t('tunnel.errors.localConnectFailed', { target }))
      } else if (code === 'socks4') {
        setError(t('tunnel.errors.socks4'))
      } else if (code === 'http_proxy') {
        setError(t('tunnel.errors.httpProxy'))
      } else if (code === 'bad_greeting') {
        setError(t('tunnel.errors.badGreeting', { detail: error }))
      } else {
        setError(error)
      }
    })

    return () => {
      unlistenStatus.then(fn => fn())
      unlistenError.then(fn => fn())
    }
  }, [sessionId, loadTunnels, fetchGatewayPorts, patchTunnel])

  const isValidPort = (port: string): boolean => {
    const num = parseInt(port)
    return !isNaN(num) && num >= 1 && num <= 65535
  }

  const isPortInUse = (port: number): boolean => {
    // remote 转发的 local_port 是出站连接目标，不是本地监听端口，无冲突
    if (tunnelType === 'remote') return false
    return tunnels.some(t => t.status === 'active' &&
      (t.tunnel_type === 'local' || t.tunnel_type === 'dynamic') &&
      t.local_port === port && t.local_host === localHost)
  }

  const handleCreate = async () => {
    if (!sessionId) return

    if (!localPort || !isValidPort(localPort)) {
      setError(t('tunnel.invalidPort'))
      return
    }
    if (isPortInUse(parseInt(localPort))) {
      setError(t('tunnel.portInUse'))
      return
    }
    if (tunnelType !== 'dynamic' && (!remotePort || !isValidPort(remotePort))) {
      setError(t('tunnel.invalidRemotePort'))
      return
    }

    setCreating(true)
    setError('')

    try {
      const tunnelId = await invoke<string>('tunnel_create', {
        sessionId,
        tunnelType,
        localHost,
        localPort: parseInt(localPort),
        remoteHost: tunnelType === 'dynamic' ? '' : remoteHost,
        remotePort: tunnelType === 'dynamic' ? 0 : parseInt(remotePort),
        note,
      })
      setMsg(t('tunnel.created'))
      setShowCreate(false)
      resetForm()
      // 本地插入新行（created_at 最大 → 与后端升序排序一致，排最后），避免全量刷新
      const newTunnel: TunnelInfo = {
        id: tunnelId,
        session_id: sessionId,
        tunnel_type: tunnelType,
        local_host: localHost,
        local_port: parseInt(localPort),
        remote_host: tunnelType === 'dynamic' ? '' : remoteHost,
        remote_port: tunnelType === 'dynamic' ? 0 : parseInt(remotePort),
        status: 'active',
        created_at: Date.now(),
        note,
      }
      setTunnels(prev => [...prev, newTunnel])
    } catch (e) {
      setError(String(e))
    } finally {
      setCreating(false)
    }
  }

  const handleClose = async (tunnelId: string) => {
    try {
      await invoke('tunnel_close', { tunnelId })
      setMsg(t('tunnel.closed'))
      patchTunnel(tunnelId, { status: 'stopped' })
    } catch (e) {
      setError(String(e))
    }
  }

  const handleBatchClose = async () => {
    const running = tunnels.filter(t => selectedIds.includes(t.id) && t.status === 'active')
    if (running.length === 0) return
    setClosingBatch(true)
    setError('')
    try {
      await invoke('tunnel_close_batch', { ids: running.map(t => t.id) })
      setMsg(t('tunnel.batchClosed', { count: running.length }))
      running.forEach(t => patchTunnel(t.id, { status: 'stopped' }))
    } catch (e) {
      setError(String(e))
    } finally {
      setClosingBatch(false)
    }
  }

  const handleRestore = async (tunnelId: string) => {
    if (!sessionId) return
    setRestoringId(tunnelId)
    setError('')
    try {
      await invoke('tunnel_restore', { sessionId, tunnelId })
      setMsg(t('tunnel.restored'))
      patchTunnel(tunnelId, { status: 'active' })
    } catch (e) {
      setError(String(e))
    } finally {
      setRestoringId(null)
    }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    try {
      await invoke('tunnel_delete', { tunnelId: deleteTarget.id })
      setMsg(t('tunnel.deleted'))
      removeTunnels([deleteTarget.id])
      setDeleteTarget(null)
      setDeleteInput('')
    } catch (e) {
      setError(String(e))
    }
  }

  const handleToggleSelect = (id: string) => {
    setSelectedIds(prev => prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id])
  }

  const handleToggleSelectAll = () => {
    setSelectedIds(prev => prev.length === tunnels.length ? [] : tunnels.map(t => t.id))
  }

  const handleBatchDelete = async () => {
    if (!deleteBatchTarget || deleteBatchTarget.length === 0) return
    setDeletingBatch(true)
    setError('')
    try {
      await invoke('tunnel_delete_batch', { ids: deleteBatchTarget.map(t => t.id) })
      setMsg(t('tunnel.batchDeleted', { count: deleteBatchTarget.length }))
      removeTunnels(deleteBatchTarget.map(t => t.id))
      setDeleteBatchTarget(null)
      setBatchDeleteInput('')
      setSelectedIds([])
    } catch (e) {
      setError(String(e))
    } finally {
      setDeletingBatch(false)
    }
  }

  const handleBatchRestore = async () => {
    if (!sessionId) return
    const stopped = tunnels.filter(t => selectedIds.includes(t.id) && t.status === 'stopped')
    if (stopped.length === 0) return
    setRestoringBatch(true)
    setError('')
    try {
      await invoke('tunnel_restore_batch', { sessionId, ids: stopped.map(t => t.id) })
      setMsg(t('tunnel.batchRestored', { count: stopped.length }))
      stopped.forEach(t => patchTunnel(t.id, { status: 'active' }))
    } catch (e) {
      setError(String(e))
    } finally {
      setRestoringBatch(false)
    }
  }

  const resetForm = () => {
    setTunnelType('local')
    setLocalHost('127.0.0.1')
    setLocalPort('')
    setRemoteHost('127.0.0.1')
    setRemotePort('')
    setNote('')
    setError('')
    setDlgGpMsg('')
  }

  const startEditNote = (tunnel: TunnelInfo) => {
    setEditNote(tunnel.note)
    setEditingId(tunnel.id)
  }

  const handleSaveNote = async (tunnelId: string) => {
    setSavingNote(true)
    setError('')
    try {
      await invoke('tunnel_update_note', { tunnelId, note: editNote })
      setMsg(t('tunnel.noteSaved'))
      setEditingId(null)
      patchTunnel(tunnelId, { note: editNote })
    } catch (e) {
      setError(String(e))
    } finally {
      setSavingNote(false)
    }
  }

  // 映射列主地址（用户的连接入口），点击可复制
  const getTunnelEndpoint = (tunnel: TunnelInfo): string => {
    switch (tunnel.tunnel_type) {
      case 'local':
      case 'dynamic':
        return `${tunnel.local_host}:${tunnel.local_port}`
      case 'remote':
        return `${tunnel.remote_host}:${tunnel.remote_port}`
      default:
        return ''
    }
  }

  // 映射列对端/说明（括号部分）
  const getTunnelSecondary = (tunnel: TunnelInfo): string => {
    switch (tunnel.tunnel_type) {
      case 'local':
        return `(${tunnel.remote_host}:${tunnel.remote_port})`
      case 'remote':
        return `(${tunnel.local_host}:${tunnel.local_port})`
      case 'dynamic':
        return '(SOCKS5)'
      default:
        return ''
    }
  }

  const getTunnelDescription = (tunnel: TunnelInfo) =>
    `${getTunnelEndpoint(tunnel)} ${getTunnelSecondary(tunnel)}`

  const quickActions = [
    { label: t('tunnel.quick.mysql'), port: 3306 },
    { label: t('tunnel.quick.redis'), port: 6379 },
    { label: t('tunnel.quick.postgres'), port: 5432 },
    { label: t('tunnel.quick.mongodb'), port: 27017 },
  ]

  const handleQuickAction = (port: number) => {
    setTunnelType('local')
    setLocalHost('127.0.0.1')
    setLocalPort(String(port))
    setRemoteHost('127.0.0.1')
    setRemotePort(String(port))
    setShowCreate(true)
  }

  const getTypeColor = (type: string): string => {
    switch (type) {
      case 'local': return '#2196F3'
      case 'remote': return '#FF9800'
      case 'dynamic': return '#9C27B0'
      default: return '#666'
    }
  }

  if (!sessionId) {
    return (
      <div className="panel-container">
        <div className="panel-header">
          <h2>{t('tunnel.title')}</h2>
        </div>
        <div className="alert alert-error">{t('tunnel.notConnected')}</div>
      </div>
    )
  }

  const canSubmit = localPort !== '' && isValidPort(localPort) && !isPortInUse(parseInt(localPort)) &&
    (tunnelType === 'dynamic' || (remotePort !== '' && isValidPort(remotePort)))

  const restorableCount = tunnels.filter(t => selectedIds.includes(t.id) && t.status === 'stopped').length
  const closableCount = tunnels.filter(t => selectedIds.includes(t.id) && t.status === 'active').length

  return (
    <div className="panel-container">
      {/* Header */}
      <div className="panel-header">
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <h2>{t('tunnel.title')}</h2>
          {tunnels.length > 0 && (
            <span style={{ fontSize: '12px', color: 'var(--green)', fontWeight: 'bold' }}>
              {tunnels.filter(t => t.status === 'active').length} {t('tunnel.active')}
            </span>
          )}
        </div>
        <div style={{ display: 'flex', gap: '8px' }}>
          <button className="btn-secondary" onClick={loadTunnels} disabled={loading}>
            {t('common.refresh')}
          </button>
          <button className="btn-primary" onClick={() => { resetForm(); setShowCreate(true) }}>
            {t('tunnel.create')}
          </button>
        </div>
      </div>

      {/* Messages */}
      {msg && (
        <div className="alert alert-success">{msg}</div>
      )}
      {error && (
        <div className="alert alert-error">{error}</div>
      )}

      {/* What is a tunnel — plain-language intro */}
      <div style={{
        background: 'var(--bg)',
        border: '1px solid var(--border)',
        borderRadius: '6px',
        padding: '12px',
        marginBottom: '16px',
      }}>
        <p style={{ fontSize: '13px', color: 'var(--text)', margin: '0 0 10px 0', lineHeight: 1.6 }}>
          💡 {t('tunnel.whatIs')}
        </p>
      </div>

      {/* Quick Actions */}
      <div className="toolbar">
        <span style={{ fontSize: '13px', color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>
          {t('tunnel.quickActions')}
        </span>
        {quickActions.map(action => (
          <button
            key={action.port}
            className="btn-secondary"
            onClick={() => handleQuickAction(action.port)}
          >
            {action.label} <span style={{ color: 'var(--text-muted)' }}>:{action.port}</span>
          </button>
        ))}
      </div>

      {/* Tunnel Table */}
      <div className="table-wrapper">
        <table className="data-table">
          <thead>
            <tr>
              <th style={{ width: '32px' }}>
                <input
                  type="checkbox"
                  checked={tunnels.length > 0 && selectedIds.length === tunnels.length}
                  onChange={handleToggleSelectAll}
                  title={t('tunnel.selectAll')}
                  style={{ cursor: 'pointer' }}
                />
              </th>
              <th>{t('common.status')}</th>
              <th>{t('tunnel.type')}</th>
              <th>{t('tunnel.mapping')}</th>
              <th>{t('tunnel.note')}</th>
              <th>{t('common.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr>
                <td colSpan={6} style={{ textAlign: 'center', padding: '2rem' }}>
                  {t('common.loading')}
                </td>
              </tr>
            ) : tunnels.length === 0 ? (
              <tr>
                <td colSpan={6} style={{ textAlign: 'center', padding: '2rem' }}>
                  <div>{t('tunnel.empty')}</div>
                  <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '4px' }}>
                    {t('tunnel.emptyHint')}
                  </div>
                </td>
              </tr>
            ) : (
              tunnels.map(tunnel => (
                <tr key={tunnel.id}>
                  <td style={{ textAlign: 'center' }}>
                    <input
                      type="checkbox"
                      checked={selectedIds.includes(tunnel.id)}
                      onChange={() => handleToggleSelect(tunnel.id)}
                      style={{ cursor: 'pointer' }}
                    />
                  </td>
                  <td>
                    {tunnel.status === 'active' ? (
                      <>
                        <span style={{
                          display: 'inline-block',
                          width: '8px',
                          height: '8px',
                          borderRadius: '50%',
                          backgroundColor: 'var(--green)',
                          marginRight: '6px',
                        }} />
                        {t('tunnel.running')}
                      </>
                    ) : (
                      <>
                        <span style={{
                          display: 'inline-block',
                          width: '8px',
                          height: '8px',
                          borderRadius: '50%',
                          backgroundColor: 'var(--text-muted)',
                          marginRight: '6px',
                        }} />
                        {t('tunnel.stopped')}
                      </>
                    )}
                  </td>
                  <td>
                    <span style={{
                      display: 'inline-block',
                      padding: '2px 8px',
                      borderRadius: '3px',
                      backgroundColor: getTypeColor(tunnel.tunnel_type),
                      color: '#fff',
                      fontSize: '12px',
                    }}>
                      {t(`tunnel.types.${tunnel.tunnel_type}`)}
                    </span>
                  </td>
                  <td style={{ fontFamily: 'monospace', fontSize: '13px', whiteSpace: 'nowrap' }}>
                    <span style={{ fontWeight: 'bold', fontSize: '13px', color: 'var(--accent)' }}>
                      {getTunnelEndpoint(tunnel)}
                    </span>
                    <span style={{ fontWeight: 'normal', fontSize: '12px', color: 'var(--text-muted)' }}>
                      {' '}{getTunnelSecondary(tunnel)}
                    </span>
                  </td>
                  <td style={{ maxWidth: '180px' }}>
                    {editingId === tunnel.id ? (
                      <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
                        <input
                          type="text"
                          value={editNote}
                          onChange={e => setEditNote(e.target.value)}
                          maxLength={100}
                          className="form-input"
                          style={{ padding: '3px 6px', fontSize: '12px', width: '110px' }}
                          autoFocus
                          onKeyDown={e => {
                            if (e.key === 'Enter') handleSaveNote(tunnel.id)
                            if (e.key === 'Escape') setEditingId(null)
                          }}
                        />
                        <button
                          className="btn-primary"
                          style={{ padding: '3px 8px', fontSize: '12px', flexShrink: 0 }}
                          onClick={() => handleSaveNote(tunnel.id)}
                          disabled={savingNote}
                        >
                          {savingNote ? t('common.saving') : t('common.save')}
                        </button>
                        <button
                          className="btn-secondary"
                          style={{ padding: '3px 8px', fontSize: '12px', flexShrink: 0 }}
                          onClick={() => setEditingId(null)}
                        >
                          {t('common.cancel')}
                        </button>
                      </div>
                    ) : (
                      <div style={{ display: 'flex', alignItems: 'center', gap: '6px', minWidth: 0 }}>
                        <span
                          style={{
                            fontSize: '12px',
                            color: tunnel.note ? 'var(--text)' : 'var(--text-muted)',
                            whiteSpace: 'nowrap',
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            maxWidth: '130px',
                            cursor: 'pointer',
                          }}
                          title={tunnel.note || t('tunnel.addNote')}
                          onClick={() => startEditNote(tunnel)}
                        >
                          {tunnel.note || '—'}
                        </span>
                        <button
                          type="button"
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            background: 'none',
                            border: 'none',
                            padding: '2px',
                            cursor: 'pointer',
                            flexShrink: 0,
                            lineHeight: 0,
                          }}
                          title={t('tunnel.editNote')}
                          aria-label={t('tunnel.editNote')}
                          onClick={() => startEditNote(tunnel)}
                        >
                          <svg width="14" height="14" viewBox="0 0 16 16" style={{ display: 'block' }}>
                            <rect x="0.6" y="5" width="1.8" height="6" rx="0.6" fill="#F472B6" />
                            <rect x="2.4" y="4.5" width="1.1" height="7" fill="#CBD5E1" />
                            <rect x="3.5" y="4.3" width="5.6" height="7.4" fill="#F59E0B" />
                            <path d="M9.1 4.3 L15.4 8 L9.1 11.7 Z" fill="#B45309" />
                          </svg>
                        </button>
                      </div>
                    )}
                  </td>
                  <td>
                    {tunnel.status === 'active' ? (
                      <>
                        <button
                          className="btn-secondary"
                          style={{ padding: '4px 10px', fontSize: '12px' }}
                          onClick={() => handleClose(tunnel.id)}
                        >
                          {t('common.close')}
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          className="btn-primary"
                          style={{ padding: '4px 10px', fontSize: '12px', marginRight: '4px' }}
                          onClick={() => handleRestore(tunnel.id)}
                          disabled={restoringId !== null}
                        >
                          {restoringId === tunnel.id ? t('tunnel.restoring') : t('tunnel.restore')}
                        </button>
                        <button
                          className="btn-secondary"
                          style={{ padding: '4px 10px', fontSize: '12px' }}
                          onClick={() => { setDeleteInput(''); setDeleteTarget(tunnel) }}
                        >
                          {t('tunnel.delete')}
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Batch actions — bottom-left of the list, always visible, grey until selection */}
      <div style={{ display: 'flex', justifyContent: 'flex-start', gap: '8px', marginTop: '12px' }}>
        <button
          className="btn-secondary"
          disabled={closableCount === 0 || closingBatch}
          style={closableCount > 0
            ? { backgroundColor: 'var(--yellow)', borderColor: 'var(--yellow)', color: '#fff' }
            : { opacity: 0.6 }}
          onClick={handleBatchClose}
        >
          {closingBatch ? t('tunnel.closing') : `${t('tunnel.batchClose')} (${closableCount})`}
        </button>
        <button
          className="btn-secondary"
          disabled={restorableCount === 0 || restoringBatch}
          style={restorableCount > 0
            ? { backgroundColor: 'var(--green)', borderColor: 'var(--green)', color: '#fff' }
            : { opacity: 0.6 }}
          onClick={handleBatchRestore}
        >
          {restoringBatch ? t('tunnel.restoring') : `${t('tunnel.batchRestore')} (${restorableCount})`}
        </button>
        <button
          className="btn-secondary"
          disabled={selectedIds.length === 0}
          style={selectedIds.length > 0
            ? { backgroundColor: 'var(--red)', borderColor: 'var(--red)', color: '#fff' }
            : { opacity: 0.6 }}
          onClick={() => {
            if (selectedIds.length === 0) return
            setBatchDeleteInput('')
            setDeleteBatchTarget(tunnels.filter(t => selectedIds.includes(t.id)))
          }}
        >
          {t('tunnel.batchDelete')} ({selectedIds.length})
        </button>
      </div>

      {/* Create Dialog */}
      {showCreate && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button
              className="modal-close-btn"
              onClick={() => setShowCreate(false)}
              title="Close"
            >×</button>
            <h3>{t('tunnel.create')}</h3>

            {/* Tunnel Type */}
            <div className="form-group">
              <div style={{ display: 'flex', gap: '6px' }}>
                {(['local', 'remote', 'dynamic'] as TunnelType[]).map(type => (
                  <button
                    key={type}
                    className={tunnelType === type ? 'btn-primary' : 'btn-secondary'}
                    style={{ padding: '6px 12px', fontSize: '12px' }}
                    onClick={() => {
                      setTunnelType(type)
                      // 服务器转发：GatewayPorts 开启 → 服务器地址为当前服务器IP，否则 127.0.0.1
                      if (type === 'remote') {
                        setRemoteHost(gpStatus?.enabled ? (serverHost || '127.0.0.1') : '127.0.0.1')
                      }
                    }}
                  >
                    {t(`tunnel.types.${type}`)}
                  </button>
                ))}
              </div>
            </div>

            {/* Local Settings */}
            <div className="form-row">
              <div className="form-group">
                <label>{t('tunnel.localHost')}:</label>
                <input
                  type="text"
                  value={localHost}
                  onChange={e => setLocalHost(e.target.value)}
                  className="form-input"
                  placeholder="127.0.0.1"
                />
              </div>
              <div className="form-group">
                <label>{t('tunnel.localPort')}:</label>
                <input
                  type="number"
                  value={localPort}
                  onChange={e => setLocalPort(e.target.value)}
                  min="1"
                  max="65535"
                  className="form-input"
                  placeholder="3306"
                />
              </div>
            </div>

            {/* Remote Settings */}
            {tunnelType !== 'dynamic' && (
              <div className="form-row">
                <div className="form-group">
                  <label>{t('tunnel.remoteHost')}:</label>
                  <input
                    type="text"
                    value={remoteHost}
                    onChange={e => setRemoteHost(e.target.value)}
                    className="form-input"
                    placeholder="127.0.0.1"
                  />
                </div>
                <div className="form-group">
                  <label>{t('tunnel.remotePort')}:</label>
                  <input
                    type="number"
                    value={remotePort}
                    onChange={e => setRemotePort(e.target.value)}
                    min="1"
                    max="65535"
                    className="form-input"
                    placeholder="3306"
                  />
                </div>
              </div>
            )}

            {/* GatewayPorts — public access check for server forwarding */}
            {tunnelType === 'remote' && (
              <div className="form-group">
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '6px', flexWrap: 'wrap', gap: '4px' }}>
                  <span style={{ fontSize: '13px', fontWeight: 'bold', color: 'var(--text)' }}>
                    🌐 {t('tunnel.gatewayPortsDialogTitle')}
                  </span>
                </div>

                {gpStatus === null ? (
                  <div style={{
                    display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                    gap: '8px', background: 'var(--bg)', border: '1px solid var(--border)',
                    borderRadius: '6px', padding: '10px 12px', flexWrap: 'wrap',
                  }}>
                    <span style={{ fontSize: '12px', color: 'var(--text-muted)', lineHeight: 1.6 }}>
                      {t('tunnel.gatewayPortsDialogUnknown')}
                    </span>
                    <button
                      className="btn-primary"
                      style={{ padding: '4px 12px', fontSize: '12px', flexShrink: 0 }}
                      onClick={handleEnableGatewayFromDialog}
                      disabled={gpSaving}
                    >
                      {gpSaving ? t('tunnel.gatewayPortsEnabling') : t('tunnel.gatewayPortsEnableBtn')}
                    </button>
                  </div>
                ) : gpStatus.enabled ? (
                  <div style={{
                    display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                    gap: '8px', background: 'rgba(35, 134, 54, 0.1)',
                    border: '1px solid rgba(35, 134, 54, 0.3)',
                    borderRadius: '6px', padding: '10px 12px', flexWrap: 'wrap',
                  }}>
                    <span style={{ fontSize: '12px', color: 'var(--green)', lineHeight: 1.6, minWidth: 0 }}>
                      {t('tunnel.gatewayPortsDialogOnWarn')}{' '}
                      <span
                        role="link"
                        style={{ color: 'var(--accent)', cursor: 'pointer', textDecoration: 'underline', whiteSpace: 'nowrap' }}
                        onClick={handleDisableGatewayFromDialog}
                        title={t('tunnel.gatewayPortsDisableLink')}
                      >
                        {t('tunnel.gatewayPortsDisableLink')}
                      </span>
                    </span>
                  </div>
                ) : (
                  <div style={{
                    display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                    gap: '8px', background: 'rgba(218, 54, 51, 0.08)',
                    border: '1px solid rgba(218, 54, 51, 0.3)',
                    borderRadius: '6px', padding: '10px 12px', flexWrap: 'wrap',
                  }}>
                    <span style={{ fontSize: '12px', color: 'var(--red)', lineHeight: 1.6, minWidth: 0 }}>
                      ⚠ {t('tunnel.gatewayPortsDialogOffWarn')}{' '}
                      <span
                        role="link"
                        style={{ color: 'var(--accent)', cursor: 'pointer', textDecoration: 'underline', whiteSpace: 'nowrap' }}
                        onClick={handleEnableGatewayFromDialog}
                        title={t('tunnel.gatewayPortsEnableLink')}
                      >
                        {t('tunnel.gatewayPortsEnableLink')}
                      </span>
                    </span>
                  </div>
                )}

                {dlgGpMsg && (
                  <div
                    style={{
                      fontSize: '12px', marginTop: '8px', padding: '8px 12px', borderRadius: '6px', lineHeight: 1.6,
                      ...(dlgGpMsgType === 'error'
                        ? { background: 'rgba(218, 54, 51, 0.1)', border: '1px solid rgba(218, 54, 51, 0.3)', color: 'var(--red)' }
                        : dlgGpMsgType === 'warn'
                          ? { background: 'rgba(210, 153, 34, 0.1)', border: '1px solid rgba(210, 153, 34, 0.35)', color: 'var(--yellow)' }
                          : { background: 'rgba(35, 134, 54, 0.1)', border: '1px solid rgba(35, 134, 54, 0.3)', color: 'var(--green)' }),
                    }}
                  >
                    {dlgGpMsg}
                  </div>
                )}
              </div>
            )}

            {/* Inline validation hints */}
            {localPort && !isValidPort(localPort) && (
              <div style={{ color: 'var(--red)', fontSize: '12px' }}>{t('tunnel.invalidPort')}</div>
            )}
            {localPort && isValidPort(localPort) && isPortInUse(parseInt(localPort)) && (
              <div style={{ color: 'var(--yellow)', fontSize: '12px' }}>{t('tunnel.portInUse')}</div>
            )}

            {/* Note */}
            <div className="form-group">
              <label>{t('tunnel.note')}:</label>
              <input
                type="text"
                value={note}
                onChange={e => setNote(e.target.value)}
                maxLength={100}
                className="form-input"
                placeholder={t('tunnel.notePlaceholder')}
              />
            </div>

            {/* Description */}
            <div style={{
              fontSize: '12px',
              color: 'var(--text-muted)',
              background: 'var(--bg)',
              border: '1px solid var(--border)',
              borderRadius: '6px',
              padding: '10px 12px',
              lineHeight: 1.7,
            }}>
              {t(`tunnel.desc.${tunnelType}`)}
            </div>

            {error && (
              <div className="alert alert-error" style={{ marginBottom: 0 }}>{error}</div>
            )}

            <div className="modal-actions">
              <button
                className="btn-secondary"
                onClick={() => setShowCreate(false)}
                disabled={creating}
              >
                {t('common.cancel')}
              </button>
              <button
                className="btn-primary"
                onClick={handleCreate}
                disabled={creating || !canSubmit}
              >
                {creating ? t('common.creating') : t('tunnel.create')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* GatewayPorts Reconnect Confirmation Dialog */}
      {showReconnectConfirm && (
        <div className="modal-overlay" onClick={() => !reconnecting && setShowReconnectConfirm(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button
              className="modal-close-btn"
              onClick={() => setShowReconnectConfirm(false)}
              disabled={reconnecting}
              title="Close"
            >×</button>
            <h3>{t('tunnel.reconnectTitle')}</h3>

            <div style={{ fontSize: '13px', color: 'var(--text)', lineHeight: 1.7, marginBottom: '12px' }}>
              {reconnectAction === 'disable' ? t('tunnel.reconnectHintDisable') : t('tunnel.reconnectHint')}
            </div>

            {reconnectAction === 'enable' && (
              <div style={{
                fontSize: '12px', color: 'var(--yellow)', lineHeight: 1.7,
                background: 'rgba(210, 153, 34, 0.1)', border: '1px solid rgba(210, 153, 34, 0.35)',
                borderRadius: '6px', padding: '10px 12px', marginBottom: '12px',
              }}>
                {t('tunnel.reconnectRisk')}
              </div>
            )}

            <div className="modal-actions">
              <button
                className="btn-secondary"
                onClick={handleReconnectLater}
                disabled={reconnecting}
              >
                {t('tunnel.reconnectLater')}
              </button>
              <button
                className="btn-primary"
                onClick={handleReconnectAgree}
                disabled={reconnecting}
              >
                {reconnecting ? t('tunnel.reconnecting') : t('tunnel.reconnectAgree')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation Dialog */}
      {deleteTarget && (
        <div className="modal-overlay" onClick={() => setDeleteTarget(null)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button
              className="modal-close-btn"
              onClick={() => setDeleteTarget(null)}
              title="Close"
            >×</button>
            <h3>{t('tunnel.deleteTitle')}</h3>

            <div style={{
              fontSize: '12px',
              color: 'var(--text-muted)',
              background: 'var(--bg)',
              border: '1px solid var(--border)',
              borderRadius: '6px',
              padding: '10px 12px',
              marginBottom: '12px',
              lineHeight: 1.7,
              fontFamily: 'monospace',
            }}>
              {getTunnelDescription(deleteTarget)}
            </div>

            <div style={{ color: 'var(--red)', fontSize: '13px', fontWeight: 'bold', marginBottom: '12px' }}>
              {t('tunnel.deleteHint')}
            </div>

            <div className="form-group">
              <input
                type="text"
                value={deleteInput}
                onChange={e => setDeleteInput(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter' && deleteInput === 'del') handleDelete()
                }}
                className="form-input"
                placeholder={t('tunnel.deletePlaceholder')}
                autoFocus
              />
            </div>

            <div className="modal-actions">
              <button
                className="btn-secondary"
                onClick={() => setDeleteTarget(null)}
              >
                {t('common.cancel')}
              </button>
              <button
                className="btn-primary"
                style={{ backgroundColor: 'var(--red)', borderColor: 'var(--red)' }}
                onClick={handleDelete}
                disabled={deleteInput !== 'del'}
              >
                {t('tunnel.delete')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Batch Delete Confirmation Dialog */}
      {deleteBatchTarget && (
        <div className="modal-overlay" onClick={() => setDeleteBatchTarget(null)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button
              className="modal-close-btn"
              onClick={() => setDeleteBatchTarget(null)}
              title="Close"
            >×</button>
            <h3>{t('tunnel.batchDeleteTitle')}</h3>

            <div style={{ color: 'var(--red)', fontSize: '13px', fontWeight: 'bold', marginBottom: '12px' }}>
              {t('tunnel.batchDeleteHint', { count: deleteBatchTarget.length })}
            </div>

            <div style={{
              fontSize: '12px',
              color: 'var(--text-muted)',
              background: 'var(--bg)',
              border: '1px solid var(--border)',
              borderRadius: '6px',
              padding: '10px 12px',
              marginBottom: '12px',
              lineHeight: 1.7,
              fontFamily: 'monospace',
              maxHeight: '150px',
              overflowY: 'auto',
            }}>
              {deleteBatchTarget.map(tunnel => (
                <div key={tunnel.id}>{getTunnelDescription(tunnel)}</div>
              ))}
            </div>

            <div className="form-group">
              <input
                type="text"
                value={batchDeleteInput}
                onChange={e => setBatchDeleteInput(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter' && batchDeleteInput === 'del') handleBatchDelete()
                }}
                className="form-input"
                placeholder={t('tunnel.deletePlaceholder')}
                autoFocus
              />
            </div>

            <div className="modal-actions">
              <button
                className="btn-secondary"
                onClick={() => setDeleteBatchTarget(null)}
                disabled={deletingBatch}
              >
                {t('common.cancel')}
              </button>
              <button
                className="btn-primary"
                style={{ backgroundColor: 'var(--red)', borderColor: 'var(--red)' }}
                onClick={handleBatchDelete}
                disabled={batchDeleteInput !== 'del' || deletingBatch}
              >
                {deletingBatch ? t('common.deleting') : t('tunnel.delete')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
