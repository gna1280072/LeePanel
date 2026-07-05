import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

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
  const [connections, setConnections] = useState<Connection[]>([])
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<{ id: string; name: string } | null>(null)
  const [editing, setEditing] = useState<Connection | null>(null)
  const [showEditPassword, setShowEditPassword] = useState(false)
  const [creating, setCreating] = useState<NewConnectionData | null>(null)
  const [showCreatePassword, setShowCreatePassword] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

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

  const loadConnections = async () => {
    const list = await invoke<Connection[]>('config_list')
    setConnections(list)
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
      remember_me: false
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
        <h2>Servers</h2>
        <button className="btn-new" onClick={handleNewConnection} title="New Connection">
          +
        </button>
      </div>
      <div className="connection-list">
        {connections.length === 0 && (
          <p className="empty-hint">Click + to add a server</p>
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
              title="Double-click to connect quickly"
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
                  title={isConnected ? 'Disconnect' : 'Connect'}
                  disabled={connectingServerId === conn.id}
                >
                  {connectingServerId === conn.id ? 'Connecting...' : (isConnected ? 'Disconnect' : 'Connect')}
                </button>
                <button
                  className="btn-edit"
                  onClick={async (e) => {
                    e.stopPropagation()
                    const list = await invoke<Connection[]>('config_list')
                    const fresh = list.find(c => c.id === conn.id)
                    setEditing(fresh ? { ...fresh } : { ...conn })
                  }}
                  title="Edit"
                >
                  Edit
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
            ⚡ Connect
          </div>
          <div
            className="context-menu-item"
            onClick={() => {
              setEditing({ ...contextMenu.conn })
              setContextMenu(null)
            }}
          >
             Edit
          </div>
          <div className="context-menu-divider" />
          <div
            className="context-menu-item danger"
            onClick={() => {
              setConfirmDelete({ id: contextMenu.conn.id, name: contextMenu.conn.name || contextMenu.conn.host })
              setContextMenu(null)
            }}
          >
            🗑 Delete
          </div>
        </div>
      )}
      {/* Edit Modal */}
      {editing && (
        <div className="sidebar-confirm-overlay" onClick={() => setEditing(null)}>
          <div className="sidebar-edit-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="sidebar-edit-header">
              <div className="sidebar-confirm-title">Edit Connection</div>
              <button className="sidebar-edit-close" onClick={() => setEditing(null)}>×</button>
            </div>
            <div className="sidebar-edit-fields">
              <label>Name</label>
              <input className="sidebar-edit-input" value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} />
              <div style={{ display: 'flex', gap: 8 }}>
                <div style={{ flex: 1 }}>
                  <label>Host</label>
                  <input className="sidebar-edit-input" value={editing.host} onChange={(e) => setEditing({ ...editing, host: e.target.value })} />
                </div>
                <div style={{ width: 80 }}>
                  <label>Port</label>
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
              <label>Username</label>
              <input className="sidebar-edit-input" value={editing.username} onChange={(e) => setEditing({ ...editing, username: e.target.value })} />
              <label>Auth Type</label>
              <select className="sidebar-edit-input" value={editing.auth_type} onChange={(e) => setEditing({ ...editing, auth_type: e.target.value, key_path: e.target.value === 'key' ? editing.key_path : undefined, password: e.target.value === 'password' ? editing.password : undefined })}>
                <option value="password">Password</option>
                <option value="key">Key File</option>
              </select>
              {editing.auth_type === 'password' && (
                <>
                  <label>Password</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} type={showEditPassword ? 'text' : 'password'} value={editing.password || ''} onChange={(e) => setEditing({ ...editing, password: e.target.value })} />
                    <button className="sidebar-edit-action-btn" onClick={() => setShowEditPassword(!showEditPassword)} title={showEditPassword ? 'Hide password' : 'Show password'}>{showEditPassword ? '🙈' : '👁'}</button>
                  </div>
                </>
              )}
              {editing.auth_type === 'key' && (
                <>
                  <label>Key Path</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} value={editing.key_path || ''} onChange={(e) => setEditing({ ...editing, key_path: e.target.value })} />
                    <button className="sidebar-edit-action-btn" onClick={pickKeyFile} title="Browse key file">📂</button>
                  </div>
                </>
              )}
            </div>
            <div className="sidebar-confirm-actions">
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', marginRight: 'auto' }}>
                <input type="checkbox" checked={editing.remember_me || false} onChange={(e) => setEditing({ ...editing, remember_me: e.target.checked })} />
                <span style={{ color: 'red' }}>Remember me</span>
              </label>
              <button className="sidebar-confirm-btn cancel" onClick={() => setEditing(null)}>Cancel</button>
              <button className="sidebar-confirm-btn primary" onClick={handleSaveEdit}>Save</button>
            </div>
          </div>
        </div>
      )}
      {/* Confirm Delete Dialog */}
      {confirmDelete && (
        <div className="sidebar-confirm-overlay" onClick={() => setConfirmDelete(null)}>
          <div className="sidebar-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="sidebar-confirm-title">Confirm Delete</div>
            <div className="sidebar-confirm-msg">
              Delete <strong>{confirmDelete.name}</strong>?
            </div>
            <div className="sidebar-confirm-actions">
              <button className="sidebar-confirm-btn cancel" onClick={() => setConfirmDelete(null)}>Cancel</button>
              <button className="sidebar-confirm-btn danger" onClick={() => handleDelete(confirmDelete.id)}>Delete</button>
            </div>
          </div>
        </div>
      )}
      {/* Create New Connection Modal */}
      {creating && (
        <div className="sidebar-confirm-overlay" onClick={() => setCreating(null)}>
          <div className="sidebar-edit-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="sidebar-edit-header">
              <div className="sidebar-confirm-title">New Connection</div>
              <button className="sidebar-edit-close" onClick={() => setCreating(null)}>×</button>
            </div>
            <div className="sidebar-edit-fields">
              <label>Name</label>
              <input className="sidebar-edit-input" value={creating.name} onChange={(e) => setCreating({ ...creating, name: e.target.value })} placeholder="Server name" />
              <div style={{ display: 'flex', gap: 8 }}>
                <div style={{ flex: 1 }}>
                  <label>Host</label>
                  <input className="sidebar-edit-input" value={creating.host} onChange={(e) => setCreating({ ...creating, host: e.target.value })} placeholder="192.168.1.1" />
                </div>
                <div style={{ width: 80 }}>
                  <label>Port</label>
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
              <label>Username</label>
              <input className="sidebar-edit-input" value={creating.username} onChange={(e) => setCreating({ ...creating, username: e.target.value })} placeholder="root" />
              <label>Auth Type</label>
              <select className="sidebar-edit-input" value={creating.auth_type} onChange={(e) => setCreating({ ...creating, auth_type: e.target.value, key_path: e.target.value === 'key' ? creating.key_path : undefined, password: e.target.value === 'password' ? creating.password : undefined })}>
                <option value="password">Password</option>
                <option value="key">Key File</option>
              </select>
              {creating.auth_type === 'password' && (
                <>
                  <label>Password</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} type={showCreatePassword ? 'text' : 'password'} value={creating.password || ''} onChange={(e) => setCreating({ ...creating, password: e.target.value })} placeholder="Enter password" />
                    <button className="sidebar-edit-action-btn" onClick={() => setShowCreatePassword(!showCreatePassword)} title={showCreatePassword ? 'Hide password' : 'Show password'}>{showCreatePassword ? '' : '👁'}</button>
                  </div>
                </>
              )}
              {creating.auth_type === 'key' && (
                <>
                  <label>Key Path</label>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input className="sidebar-edit-input" style={{ flex: 1 }} value={creating.key_path || ''} onChange={(e) => setCreating({ ...creating, key_path: e.target.value })} placeholder="~/.ssh/id_rsa" />
                    <button className="sidebar-edit-action-btn" onClick={pickCreateKeyFile} title="Browse key file">📂</button>
                  </div>
                </>
              )}
            </div>
            <div className="sidebar-confirm-actions">
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', marginRight: 'auto' }}>
                <input type="checkbox" checked={creating.remember_me || false} onChange={(e) => setCreating({ ...creating, remember_me: e.target.checked })} />
                <span style={{ color: 'red' }}>Remember me</span>
              </label>
              <button className="sidebar-confirm-btn cancel" onClick={() => setCreating(null)}>Cancel</button>
              <button className="sidebar-confirm-btn primary" onClick={handleSaveNewConnection}>Create</button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

