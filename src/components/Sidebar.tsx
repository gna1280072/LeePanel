import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { useTranslation } from 'react-i18next'

interface Connection {
  id: string
  name: string
  host: string
  port: number
  username: string
  auth_type: string
  key_path?: string
  password?: string
  remember_me?: boolean
}

interface NewConnectionData {
  name: string
  host: string
  port: number
  username: string
  auth_type: string
  key_path?: string
  password?: string
  remember_me?: boolean
}

interface SidebarProps {
  onSelect: (conn: Connection) => void
  onConnect: (conn: Connection) => void
  onNew: () => void
  onCreateConnection: (data: NewConnectionData) => Promise<void>
  refreshKey?: number
  currentSessionId?: string | null
  connectingServerId?: string | null
}

interface ContextMenu {
  x: number
  y: number
  conn: Connection
}

export default function Sidebar({ onSelect, onConnect, onNew, onCreateConnection, refreshKey, currentSessionId, connectingServerId }: SidebarProps) {
  const { t, i18n } = useTranslation()
  const [connections, setConnections] = useState<Connection[]>([])
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<{ id: string; name: string } | null>(null)
  const [editing, setEditing] = useState<Connection | null>(null)
  const [showEditPassword, setShowEditPassword] = useState(false)
  const [creating, setCreating] = useState<NewConnectionData | null>(null)
  const [showCreatePassword, setShowCreatePassword] = useState(false)
  const [hasCheckedEmpty, setHasCheckedEmpty] = useState(false)
  const [langDropdownOpen, setLangDropdownOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const langRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    loadConnections()
  }, [])

  // Refresh when refreshKey changes
  useEffect(() => {
    if (refreshKey && refreshKey > 0) {
      loadConnections()
    }
  }, [refreshKey])

  // Close context menu on click outside
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setContextMenu(null)
      }
    }
    if (contextMenu) {
      window.addEventListener('mousedown', handleClick)
      return () => window.removeEventListener('mousedown', handleClick)
    }
  }, [contextMenu])

  // Close lang dropdown on click outside
  useEffect(() => {
    if (!langDropdownOpen) return
    const handleClick = (e: MouseEvent) => {
      if (langRef.current && !langRef.current.contains(e.target as Node)) {
        setLangDropdownOpen(false)
      }
    }
    window.addEventListener('mousedown', handleClick)
    return () => window.removeEventListener('mousedown', handleClick)
  }, [langDropdownOpen])

  const loadConnections = async () => {
    const list = await invoke<Connection[]>('config_list')
    setConnections(list)
    
    // Check if empty on first load only
    if (!hasCheckedEmpty && list.length === 0) {
      setCreating({
        name: '',
        host: '',
        port: 22,
        username: 'root',
        auth_type: 'password',
        password: '',
        remember_me: true
      })
      setHasCheckedEmpty(true)
    }
  }

  const handleDelete = async (id: string) => {
    await invoke('config_delete', { id })
    setConfirmDelete(null)
    loadConnections()
  }

  const handleSaveEdit = async () => {
    if (!editing) return
    // Trim whitespace from host, username, and port
    const trimmed = {
      ...editing,
      host: editing.host.trim(),
      username: editing.username.trim(),
      port: Number(String(editing.port).trim()) || editing.port,
      remember_me: editing.remember_me || false,
      // Only save credentials if remember_me is checked
      password: editing.remember_me ? editing.password : undefined,
      key_path: editing.remember_me ? editing.key_path : undefined
    }
    await invoke('config_save', { connection: trimmed })
    setEditing(null)
    loadConnections()
  }

  const handleSaveAndConnect = async () => {
    if (!editing) return
    
    // Save first
    const trimmed = {
      ...editing,
      host: editing.host.trim(),
      username: editing.username.trim(),
      port: Number(String(editing.port).trim()) || editing.port,
      remember_me: editing.remember_me || false,
      // Only save credentials if remember_me is checked
      password: editing.remember_me ? editing.password : undefined,
      key_path: editing.remember_me ? editing.key_path : undefined
    }
    await invoke('config_save', { connection: trimmed })
    
    setEditing(null)
    loadConnections()
    
    // Trigger reconnect via custom event
    window.dispatchEvent(new CustomEvent('sidebar-reconnect-after-edit', {
      detail: { conn: trimmed }
    }))
  }

  const pickKeyFile = async () => {
    const path = await open()
    if (path) setEditing({ ...editing!, key_path: String(path) })
  }

  const pickCreateKeyFile = async () => {
    const path = await open()
    if (path && creating) setCreating({ ...creating, key_path: String(path) })
  }

  const handleSaveNewConnection = async () => {
    if (!creating) return
    // Trim whitespace from host, username, and port
    const trimmed = {
      ...creating,
      host: creating.host.trim(),
      username: creating.username.trim(),
      port: Number(String(creating.port).trim()) || creating.port,
      remember_me: creating.remember_me || false,
      // Only save credentials if remember_me is checked
      password: creating.remember_me ? creating.password : undefined,
      key_path: creating.remember_me ? creating.key_path : undefined
    }
    await onCreateConnection(trimmed)
    setCreating(null)
    loadConnections()
  }

  const handleNewConnection = () => {
    setCreating({
      name: '',
      host: '',
      port: 22,
      username: 'root',
      auth_type: 'password',
      password: '',
      remember_me: true
    })
    onNew()
  }

  const handleContextMenu = (e: React.MouseEvent, conn: Connection) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY, conn })
  }

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <h2>{t('sidebar.servers')}</h2>
        <button className="btn-new" onClick={handleNewConnection} title={t('sidebar.newConnection')}>
          +
        </button>
      </div>
      <div className="connection-list">
        {connections.length === 0 && (
          <p className="empty-hint">{t('sidebar.clickToAdd')}</p>
        )}
        {connections.map((conn) => {
          const isConnected = conn.id === currentSessionId
          console.log(`Connection ${conn.name}: id=${conn.id}, currentSessionId=${currentSessionId}, isConnected=${isConnected}`)
          return (
            <div
              key={conn.id}
              className="connection-item"
              onClick={() => onSelect(conn)}
              onDoubleClick={() => onConnect(conn)}
              onContextMenu={(e) => handleContextMenu(e, conn)}
              title={t('sidebar.doubleClickHint')}
            >
              <div className="conn-info">
                <span className="conn-name">{conn.name || conn.host}</span>
                <span className="conn-detail">
                  {conn.username}@{conn.host}:{conn.port}
                </span>
              </div>
              <div style={{ display: 'flex', gap: 6 }}>
                <button
                  className={`btn-connect ${isConnected ? 'disconnect' : ''}`}
                  onClick={(e) => {
                    e.stopPropagation()
                    if (isConnected) {
                      // Disconnect - call the onDisconnect handler directly
                      window.dispatchEvent(new CustomEvent('sidebar-disconnect'))
                    } else {
                      onConnect(conn)
                    }
                  }}
                  title={isConnected ? t('common.disconnect') : t('common.connect')}
                  disabled={connectingServerId === conn.id}
                >
                  {connectingServerId === conn.id ? t('common.connecting') : (isConnected ? t('common.disconnect') : t('common.connect'))}
                </button>
                <button
                  className="btn-edit"
                  onClick={async (e) => {
                    e.stopPropagation()
                    const list = await invoke<Connection[]>('config_list')
                    const fresh = list.find(c => c.id === conn.id)
                    setEditing(fresh ? { ...fresh } : { ...conn })
                  }}
                  title={t('common.edit')}
                >
                  ✏️ {t('common.edit')}
                </button>
              </div>
            </div>
          )
        })}
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <div
          ref={menuRef}
          className="context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <div
            className="context-menu-item"
            onClick={() => {
              onConnect(contextMenu.conn)
              setContextMenu(null)
            }}
          >
            ⚡ {t('common.connect')}
          </div>
          <div
            className="context-menu-item"
            onClick={() => {
              setEditing({ ...contextMenu.conn })
              setContextMenu(null)
            }}
          >
             ✏️ {t('common.edit')}
          </div>
          <div className="context-menu-divider" />
          <div
            className="context-menu-item danger"
            onClick={() => {
              setConfirmDelete({ id: contextMenu.conn.id, name: contextMenu.conn.name || contextMenu.conn.host })
              setContextMenu(null)
            }}
          >
            🗑 {t('common.delete')}
          </div>
        </div>
      )}
      {/* Edit Modal */}
      {editing && (
        <div className="sidebar-confirm-overlay">
          <div className="sidebar-edit-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="sidebar-edit-header">
              <div className="sidebar-confirm-title">{t('sidebar.editConnection')}</div>
              <button className="sidebar-edit-close" onClick={() => setEditing(null)}>×</button>
            </div>
            <div className="sidebar-edit-fields">
              <div className="form-group">
                <label>{t('sidebar.name')}</label>
                <input className="sidebar-edit-input" value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} />
              </div>
              <div className="form-row">
                <div className="form-group">
                  <label>{t('sidebar.host')}</label>
                  <input className="sidebar-edit-input" value={editing.host} onChange={(e) => setEditing({ ...editing, host: e.target.value })} />
                </div>
                <div className="form-group fixed-width">
                  <label>{t('sidebar.port')}</label>
                  <input 
                    className="sidebar-edit-input" 
                    type="number" 
                    value={editing.port || ''} 
                    onChange={(e) => {
                      const val = e.target.value
                      setEditing({ ...editing, port: val === '' ? 0 : Number(val) })
                    }}
                    onBlur={(e) => {
                      const val = e.target.value
                      if (val === '' || val.trim() === '') {
                        setEditing({ ...editing, port: 0 })
                      }
                    }}
                  />
                </div>
              </div>
              <div className="form-row">
                <div className="form-group">
                  <label>{t('sidebar.username')}</label>
                  <input className="sidebar-edit-input" value={editing.username} onChange={(e) => setEditing({ ...editing, username: e.target.value })} />
                </div>
                <div className="form-group medium-width">
                  <label>{t('sidebar.authType')}</label>
                  <select className="sidebar-edit-input" value={editing.auth_type} onChange={(e) => setEditing({ ...editing, auth_type: e.target.value, key_path: e.target.value === 'key' ? editing.key_path : undefined, password: e.target.value === 'password' ? editing.password : undefined })}>
                    <option value="password">{t('sidebar.password')}</option>
                    <option value="key">Key File</option>
                  </select>
                </div>
              </div>
              {editing.auth_type === 'password' && (
                <div className="form-group">
                  <label>{t('sidebar.password')}</label>
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} type={showEditPassword ? 'text' : 'password'} value={editing.password || ''} onChange={(e) => setEditing({ ...editing, password: e.target.value })} />
                    <button className="sidebar-edit-action-btn" onClick={() => setShowEditPassword(!showEditPassword)} title={showEditPassword ? t('sidebar.hidePassword') : t('sidebar.showPassword')}>{showEditPassword ? '🙈' : '👁'}</button>
                  </div>
                </div>
              )}
              {editing.auth_type === 'key' && (
                <div className="form-group">
                  <label>{t('sidebar.keyPath')}</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} value={editing.key_path || ''} onChange={(e) => setEditing({ ...editing, key_path: e.target.value })} />
                    <button className="sidebar-edit-action-btn" onClick={pickKeyFile} title={t('sidebar.browseKeyFile')}>📂</button>
                  </div>
                </div>
              )}
            </div>
            <div className="sidebar-confirm-actions">
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', marginRight: 'auto' }}>
                <input type="checkbox" checked={editing.remember_me || false} onChange={(e) => setEditing({ ...editing, remember_me: e.target.checked })} />
                <span style={{ color: 'red' }}>{t('sidebar.rememberMe')}</span>
              </label>
              <button className="sidebar-confirm-btn primary" onClick={handleSaveEdit}>{t('common.save')}</button>
              <button className="sidebar-confirm-btn connect" onClick={handleSaveAndConnect}>{t('common.connect')}</button>
            </div>
          </div>
        </div>
      )}
      {/* Confirm Delete Dialog */}
      {confirmDelete && (
        <div className="sidebar-confirm-overlay" onClick={() => setConfirmDelete(null)}>
          <div className="sidebar-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="sidebar-confirm-title">{t('sidebar.confirmDelete')}</div>
            <div className="sidebar-confirm-msg">
              {t('sidebar.deleteConfirmMsg', { name: confirmDelete.name })}
            </div>
            <div className="sidebar-confirm-actions">
              <button className="sidebar-confirm-btn cancel" onClick={() => setConfirmDelete(null)}>{t('common.cancel')}</button>
              <button className="sidebar-confirm-btn danger" onClick={() => handleDelete(confirmDelete.id)}>{t('common.delete')}</button>
            </div>
          </div>
        </div>
      )}
      {/* Create New Connection Modal */}
      {creating && (
        <div className="sidebar-confirm-overlay">
          <div className="sidebar-edit-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="sidebar-edit-header">
              <div className="sidebar-confirm-title">{t('sidebar.newConnection')}</div>
              <button className="sidebar-edit-close" onClick={() => setCreating(null)}>×</button>
            </div>
            <div className="sidebar-edit-fields">
              <div className="form-group">
                <label>{t('sidebar.name')}</label>
                <input className="sidebar-edit-input" value={creating.name} onChange={(e) => setCreating({ ...creating, name: e.target.value })} placeholder={t('sidebar.serverName')} />
              </div>
              <div className="form-row">
                <div className="form-group">
                  <label>{t('sidebar.host')}</label>
                  <input className="sidebar-edit-input" value={creating.host} onChange={(e) => setCreating({ ...creating, host: e.target.value })} placeholder="192.168.1.1" />
                </div>
                <div className="form-group fixed-width">
                  <label>{t('sidebar.port')}</label>
                  <input 
                    className="sidebar-edit-input" 
                    type="number" 
                    value={creating.port || ''} 
                    onChange={(e) => {
                      const val = e.target.value
                      setCreating({ ...creating, port: val === '' ? 0 : Number(val) })
                    }}
                    onBlur={(e) => {
                      const val = e.target.value
                      if (val === '' || val.trim() === '') {
                        setCreating({ ...creating, port: 0 })
                      }
                    }}
                  />
                </div>
              </div>
              <div className="form-row">
                <div className="form-group">
                  <label>{t('sidebar.username')}</label>
                  <input className="sidebar-edit-input" value={creating.username} onChange={(e) => setCreating({ ...creating, username: e.target.value })} placeholder="root" />
                </div>
                <div className="form-group medium-width">
                  <label>{t('sidebar.authType')}</label>
                  <select className="sidebar-edit-input" value={creating.auth_type} onChange={(e) => setCreating({ ...creating, auth_type: e.target.value, key_path: e.target.value === 'key' ? creating.key_path : undefined, password: e.target.value === 'password' ? creating.password : undefined })}>
                    <option value="password">{t('sidebar.password')}</option>
                    <option value="key">Key File</option>
                  </select>
                </div>
              </div>
              {creating.auth_type === 'password' && (
                <div className="form-group">
                  <label>{t('sidebar.password')}</label>
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} type={showCreatePassword ? 'text' : 'password'} value={creating.password || ''} onChange={(e) => setCreating({ ...creating, password: e.target.value })} placeholder={t('sidebar.enterPassword')} />
                    <button className="sidebar-edit-action-btn" onClick={() => setShowCreatePassword(!showCreatePassword)} title={showCreatePassword ? t('sidebar.hidePassword') : t('sidebar.showPassword')}>{showCreatePassword ? '🙈' : '👁'}</button>
                  </div>
                </div>
              )}
              {creating.auth_type === 'key' && (
                <div className="form-group">
                  <label>{t('sidebar.keyPath')}</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} value={creating.key_path || ''} onChange={(e) => setCreating({ ...creating, key_path: e.target.value })} placeholder="~/.ssh/id_rsa" />
                    <button className="sidebar-edit-action-btn" onClick={pickCreateKeyFile} title={t('sidebar.browseKeyFile')}>📂</button>
                  </div>
                </div>
              )}
            </div>
            <div className="sidebar-confirm-actions">
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', marginRight: 'auto' }}>
                <input type="checkbox" checked={creating.remember_me || false} onChange={(e) => setCreating({ ...creating, remember_me: e.target.checked })} />
                <span style={{ color: 'red' }}>{t('sidebar.rememberMe')}</span>
              </label>
              <button className="sidebar-confirm-btn cancel" onClick={() => setCreating(null)}>{t('common.cancel')}</button>
              <button className="sidebar-confirm-btn primary" onClick={handleSaveNewConnection}>{t('common.create')}</button>
            </div>
          </div>
        </div>
      )}
      {/* Language Switcher */}
      <div className="sidebar-language-switcher" ref={langRef} style={{ position: 'relative' }}>
        <button
          className="lang-toggle-btn"
          onClick={() => setLangDropdownOpen(!langDropdownOpen)}
        >
          🌐 Language ▾
        </button>
        {langDropdownOpen && (
          <div className="lang-dropdown">
            {[
              { code: 'en', label: 'English' },
              { code: 'zh-CN', label: '简体中文' },
              { code: 'zh-TW', label: '繁體中文' },
              { code: 'ja', label: '日本語' },
              { code: 'fr', label: 'Français' },
              { code: 'de', label: 'Deutsch' },
              { code: 'ru', label: 'Русский' },
              { code: 'ar', label: 'العربية' },
              { code: 'pt', label: 'Português' },
              { code: 'ko', label: '한국어' },
            ].map(l => (
              <div
                key={l.code}
                className={`lang-dropdown-item${i18n.language === l.code ? ' active' : ''}`}
                onClick={() => {
                  i18n.changeLanguage(l.code)
                  invoke('ui_state_set', { key: 'language', value: l.code }).catch(() => {})
                  setLangDropdownOpen(false)
                }}
              >
                {l.label}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

