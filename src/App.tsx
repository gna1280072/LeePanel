import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { check } from '@tauri-apps/plugin-updater'
import Sidebar from './components/Sidebar'
import ServerPanel from './components/ServerPanel'
import type { TerminalHandle } from './components/Terminal'
import './App.css'

interface UploadItem {
  file: File
  fileName: string
  remotePath: string
  status: 'pending' | 'uploading' | 'done' | 'error' | 'stopped'
  error?: string
}

interface UploadState {
  queue: UploadItem[]
  totalBytes: number
  uploadedBytes: number
  speed: number
  active: boolean
  paused: boolean
}

interface SidebarConnection {
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

interface ConnectionConfig {
  host: string
  port: number
  username: string
  password?: string
  keyPath?: string
  rememberMe?: boolean
}

interface Settings {
  auto_reconnect: boolean
  reconnect_interval: number
  max_reconnect_attempts: number
  cache_ttl_hours: number
  cache_max_files: number
  cache_enabled: boolean
}

function App() {
  const [sessionId, setSessionId] = useState<string | null>(null)
  const [connectedConfigId, setConnectedConfigId] = useState<string | null>(null)
  const [connectingServerId, setConnectingServerId] = useState<string | null>(null)
  const [error, setError] = useState('')
  const [toast, setToast] = useState('')
  const [showWelcome, setShowWelcome] = useState(false)
  const termRef = useRef<TerminalHandle | null>(null)

  // Password prompt dialog
  const [passwordPrompt, setPasswordPrompt] = useState<{
    show: boolean
    host: string
    username: string
    port: number
    configId?: string
    retryFn: ((params: { username: string; password?: string; keyPath?: string }) => void) | null
  }>({ show: false, host: '', username: '', port: 22, configId: undefined, retryFn: null })

  // Settings
  const [settings, setSettings] = useState<Settings>({
    auto_reconnect: true, reconnect_interval: 5, max_reconnect_attempts: 10, cache_ttl_hours: 24, cache_max_files: 500, cache_enabled: true
  })
  const [, setReconnecting] = useState(false)
  const reconnectAttemptRef = useRef(0)
  const autoReconnectRef = useRef(true)
  const reconnectingRef = useRef(false)
  const manualDisconnectRef = useRef(false)
  const [sidebarRefreshKey, setSidebarRefreshKey] = useState(0)
  const [connHost, setConnHost] = useState('')

  const clearSession = () => {
    setSessionId(null)
    setConnectedConfigId(null)
    setConnHost('')
  }

  // Draggable dividers
  const [sidebarWidth, setSidebarWidth] = useState(240)
  const [sidebarVisible, setSidebarVisible] = useState(true)
  const draggingRef = useRef<'sidebar' | null>(null)
  const splitContainerRef = useRef<HTMLDivElement>(null)
  // Listen for disconnect request from Sidebar
  useEffect(() => {
    const handleDisconnectRequest = () => {
      console.log('Received sidebar-disconnect event')
      if (sessionId) {
        manualDisconnectRef.current = true
        invoke('ssh_disconnect', { sessionId }).then(() => {
          clearSession()
          // Clear terminal after clearing session state
          termRef.current?.clear()
        })
      }
    }
    window.addEventListener('sidebar-disconnect', handleDisconnectRequest)
    return () => window.removeEventListener('sidebar-disconnect', handleDisconnectRequest)
  }, [sessionId])

  // Upload queue state
  const [upload, setUpload] = useState<UploadState>({
    queue: [], totalBytes: 0, uploadedBytes: 0, speed: 0, active: false, paused: false
  })
  const uploadPauseRef = useRef(false)
  const uploadStopRef = useRef(false)
  const uploadCompleteRef = useRef<(() => void) | null>(null)

  const handleStartUpload = useCallback(async (files: { file: File; fileName: string; remotePath: string }[]) => {
    if (!sessionId || files.length === 0) return
    const sid = sessionId
    const totalBytes = files.reduce((sum, f) => sum + f.file.size, 0)
    const queue: UploadItem[] = files.map(f => ({ ...f, status: 'pending' as const }))
    setUpload({ queue, totalBytes, uploadedBytes: 0, speed: 0, active: true, paused: false })
    uploadPauseRef.current = false
    uploadStopRef.current = false

    let uploadedBytes = 0
    const startTime = Date.now()
    const CHUNK_SIZE = 1024 * 1024

    for (let i = 0; i < queue.length; i++) {
      if (uploadStopRef.current) {
        setUpload(prev => ({
          ...prev, active: false, paused: false,
          queue: prev.queue.map((q, j) => j >= i ? { ...q, status: 'stopped' } : q)
        }))
        return
      }

      const item = queue[i]
      setUpload(prev => ({
        ...prev,
        queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'uploading' } : q)
      }))

      try {
        let offset = 0
        while (offset < item.file.size) {
          // Check stop
          if (uploadStopRef.current) {
            setUpload(prev => ({
              ...prev, active: false, paused: false,
              queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'stopped' } : j > i ? { ...q, status: 'stopped' } : q)
            }))
            return
          }
          // Wait while paused
          while (uploadPauseRef.current) {
            if (uploadStopRef.current) {
              setUpload(prev => ({
                ...prev, active: false, paused: false,
                queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'stopped' } : j > i ? { ...q, status: 'stopped' } : q)
              }))
              return
            }
            await new Promise(r => setTimeout(r, 100))
          }

          const end = Math.min(offset + CHUNK_SIZE, item.file.size)
          const slice = item.file.slice(offset, end)
          const buffer = await slice.arrayBuffer()
          await invoke('ssh_upload_chunk', {
            sessionId: sid,
            remotePath: item.remotePath,
            data: new Uint8Array(buffer),
            offset,
          })
          uploadedBytes += (end - offset)
          offset = end
          const elapsed = (Date.now() - startTime) / 1000
          const speed = elapsed > 0 ? uploadedBytes / elapsed : 0
          setUpload(prev => ({ ...prev, uploadedBytes, speed }))
        }
        setUpload(prev => ({
          ...prev,
          queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'done' } : q)
        }))
      } catch (err) {
        setUpload(prev => ({
          ...prev,
          queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'error', error: String(err) } : q)
        }))
      }
    }

    setUpload(prev => ({ ...prev, active: false, paused: false }))
    uploadCompleteRef.current?.()
  }, [sessionId])

  const handlePauseUpload = useCallback(() => {
    uploadPauseRef.current = true
    setUpload(prev => ({ ...prev, paused: true }))
  }, [])

  const handleResumeUpload = useCallback(() => {
    uploadPauseRef.current = false
    setUpload(prev => ({ ...prev, paused: false }))
  }, [])

  const handleStopUpload = useCallback(() => {
    uploadStopRef.current = true
    uploadPauseRef.current = false
  }, [])

  const handleDismissUpload = useCallback(() => {
    if (upload.active) return
    setUpload({ queue: [], totalBytes: 0, uploadedBytes: 0, speed: 0, active: false, paused: false })
  }, [upload.active])

  const [jumpToPath, setJumpToPath] = useState<string | null>(null)

  const handleCreateConnection = async (data: { name: string; host: string; port: number; username: string; auth_type: string; key_path?: string; password?: string; remember_me?: boolean }) => {
    // Save the new connection
    await invoke('config_save', {
      connection: {
        id: Date.now().toString(),
        name: data.name,
        host: data.host,
        port: data.port,
        username: data.username,
        auth_type: data.auth_type,
        key_path: data.key_path,
        password: data.password,
        remember_me: data.remember_me || false,
      },
    })
    setSidebarRefreshKey(k => k + 1)
  }

  const handleUpdateSettings = async (updates: Partial<Settings>) => {
    const newSettings = { ...settings, ...updates }
    setSettings(newSettings)
    autoReconnectRef.current = newSettings.auto_reconnect
    await invoke('settings_save', { settings: newSettings }).catch(() => {})
  }


  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (draggingRef.current === 'sidebar') {
        const w = Math.max(150, Math.min(500, e.clientX))
        setSidebarWidth(w)
      }
    }
    const onMouseUp = () => {
      if (draggingRef.current) {
        draggingRef.current = null
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
      }
    }
    window.addEventListener('mousemove', onMouseMove)
    window.addEventListener('mouseup', onMouseUp)
    return () => {
      window.removeEventListener('mousemove', onMouseMove)
      window.removeEventListener('mouseup', onMouseUp)
    }
  }, [])

  const startDrag = (type: 'sidebar') => {
    draggingRef.current = type
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }

  // Load settings on mount
  useEffect(() => {
    invoke<Settings>('settings_load').then(s => {
      setSettings(s)
      autoReconnectRef.current = s.auto_reconnect
    }).catch(() => {})
    // ponytail: auto-check for updates on startup, silent if no update or offline
    check().then(update => {
      if (update?.available) {
        update.downloadAndInstall().catch(console.error)
      }
    }).catch(() => {})
  }, [])

  const toggleAutoReconnect = async () => {
    const newSettings = { ...settings, auto_reconnect: !settings.auto_reconnect }
    setSettings(newSettings)
    autoReconnectRef.current = newSettings.auto_reconnect
    await invoke('settings_save', { settings: newSettings }).catch(() => {})
  }

  useEffect(() => {
    // Keep ref in sync
    autoReconnectRef.current = settings.auto_reconnect
  }, [settings.auto_reconnect])

  // Listen for ssh-disconnected event
  useEffect(() => {
    const unlisten = listen<{ sessionId: string; reason: string }>('ssh-disconnected', async (event) => {
      const sid = event.payload.sessionId
      // Skip auto-reconnect if user manually disconnected
      if (manualDisconnectRef.current) {
        clearSession()
        return
      }
      if (sid && autoReconnectRef.current && !reconnectingRef.current) {
        reconnectingRef.current = true
        setReconnecting(true)
        reconnectAttemptRef.current = 0
        showToast('⚠ Connection lost. Reconnecting...')

        const attemptReconnect = async () => {
          reconnectAttemptRef.current++
          if (reconnectAttemptRef.current > settings.max_reconnect_attempts) {
            showToast(`✗ Reconnect failed after ${settings.max_reconnect_attempts} attempts`)
            reconnectingRef.current = false
            setReconnecting(false)
            clearSession()
            return
          }
          try {
            await invoke('ssh_reconnect', { sessionId: sid })
            showToast(`✓ Reconnected (attempt ${reconnectAttemptRef.current})`)
            reconnectingRef.current = false
            setReconnecting(false)
          } catch {
            showToast(`↻ Reconnect attempt ${reconnectAttemptRef.current}/${settings.max_reconnect_attempts}...`)
            setTimeout(attemptReconnect, settings.reconnect_interval * 1000)
          }
        }
        setTimeout(attemptReconnect, settings.reconnect_interval * 1000)
      } else if (!autoReconnectRef.current) {
        showToast(' Connection lost')
        clearSession()
      }
    })
    return () => { unlisten.then((fn) => fn()) }
  }, [settings]) // eslint-disable-line

  useEffect(() => {
    const unlisten = listen<string>('ssh-closed', (event) => {
      if (sessionId && event.payload === sessionId) {
        clearSession()
      }
    })
    return () => { unlisten.then((fn) => fn()) }
  }, [sessionId])

  const showToast = (msg: string) => {
    setToast(msg)
    setTimeout(() => setToast(''), 4000)
  }

  // Disconnect SSH session after LNMP installation (environment changes require fresh session)
  const handleDisconnectAfterInstall = async () => {
    if (!sessionId) return
    try {
      showToast('✓ Installation complete. Disconnecting to refresh environment...')
      manualDisconnectRef.current = true
      await invoke('ssh_disconnect', { sessionId })
      clearSession()
      termRef.current?.clear()
    } catch (e) {
      console.error('Disconnect failed:', e)
      showToast(`⚠️ Disconnect failed: ${String(e).slice(0, 100)}`)
    }
  }

  // Check if a connection with same host/port/username already exists
  const findDuplicateConn = async (host: string, port: number, username: string): Promise<string | null> => {
    try {
      const conns = await invoke<SidebarConnection[]>('config_list')
      const dup = conns.find(c => c.host === host && c.port === port && c.username === username)
      return dup?.id ?? null
    } catch { return null }
  }

  const isAuthError = (e: string) => {
    const s = e.toLowerCase()
    return s.includes('auth failed') || s.includes('auth error') || s.includes('authentication') || s.includes('no authentication') || s.includes('permission denied')
  }

  const handleConnect = async (config: ConnectionConfig) => {
    setConnectingServerId(config.host + ':' + config.port)
    setError('')

    // Save connection immediately on Connect click, regardless of outcome
    let configId: string | null = null
    try {
      const existingId = await findDuplicateConn(config.host, config.port, config.username)
      if (!existingId) {
        configId = Date.now().toString()
        await invoke('config_save', {
          connection: {
            id: configId,
            name: config.host,
            host: config.host,
            port: config.port,
            username: config.username,
            auth_type: config.keyPath ? 'key' : 'password',
            key_path: config.keyPath,
            password: config.password,
          },
        })
        setSidebarRefreshKey(k => k + 1)
      } else {
        configId = existingId
        // Update existing connection's auth info
        const conns = await invoke<SidebarConnection[]>('config_list')
        const existing = conns.find(c => c.id === existingId)
        if (existing) {
          await invoke('config_save', {
            connection: {
              ...existing,
              auth_type: config.keyPath ? 'key' : 'password',
              key_path: config.keyPath,
              password: config.password,
            },
          })
        }
      }
    } catch (e) { console.error('Save connection failed:', e) }

    try {
      const sid = await invoke<string>('ssh_connect', {
        config: {
          host: config.host,
          port: config.port,
          username: config.username,
          password: config.password,
          keyPath: config.keyPath,
        },
      })
      setSessionId(sid)
      setConnectedConfigId(configId)
      console.log('Connected! sessionId:', sid, 'configId:', configId)
      setConnHost(`${config.host}_${config.port}`)
      manualDisconnectRef.current = false
      showToast(`✓ Connected`)
      // Show welcome modal on first connection or new session
      setShowWelcome(true)
      console.log('Showing welcome modal')
      setTimeout(() => {
        setShowWelcome(false)
        console.log('Hiding welcome modal after 4 seconds')
      }, 4000)
    } catch (e) {
      const errMsg = String(e)
      if (isAuthError(errMsg)) {
        setPasswordPrompt({
          show: true,
          host: config.host,
          username: config.username,
          port: config.port,
          configId: configId ?? undefined,
          retryFn: (params) => {
            setPasswordPrompt(p => ({ ...p, show: false }))
            handleConnect({ ...config, username: params.username, password: params.password, keyPath: params.keyPath })
          },
        })
      } else {
        setError(errMsg)
      }
    } finally {
      setConnectingServerId(null)
    }
  }

  const handleSelectConnection = (_conn: SidebarConnection) => {
    // ponytail: no-op, kept for interface compatibility
  }

  const showAuthDialog = (host: string, port: number, username: string, configId: string | undefined, onRetry: (params: { username: string; password?: string; keyPath?: string }) => void) => {
    setPasswordPrompt({ show: true, host, username, port, configId, retryFn: (params) => { setPasswordPrompt(p => ({ ...p, show: false })); onRetry(params) } })
  }

  const handleDirectConnect = async (conn: SidebarConnection) => {
    // If already connected, disconnect first (mark as manual to prevent auto-reconnect)
    if (sessionId) {
      manualDisconnectRef.current = true
      await invoke('ssh_disconnect', { sessionId }).catch(() => {})
      clearSession()
    }

    console.log('Direct connect config:', {
      host: conn.host,
      port: conn.port,
      username: conn.username,
      auth_type: conn.auth_type,
      has_password: !!conn.password,
      has_key_path: !!conn.key_path,
      remember_me: conn.remember_me,
    })

    const doConnect = (username: string, password?: string, keyPath?: string) => {
      setConnectingServerId(conn.id)
      setError('')
      invoke<string>('ssh_connect', {
        config: { host: conn.host, port: conn.port, username, password, keyPath },
      }).then(sid => {
        setSessionId(sid)
        setConnectedConfigId(conn.id)
        console.log('Direct connect! sessionId:', sid, 'configId:', conn.id)
        setConnHost(`${conn.host}_${conn.port}`)
        manualDisconnectRef.current = false
        // Show welcome modal on successful connection
        setShowWelcome(true)
        console.log('Showing welcome modal (direct connect)')
        setTimeout(() => {
          setShowWelcome(false)
          console.log('Hiding welcome modal after 4 seconds')
        }, 4000)
      }).catch(e => {
        const msg = String(e)
        if (isAuthError(msg)) {
          showAuthDialog(conn.host, conn.port, username, conn.id, (params) => doConnect(params.username, params.password, params.keyPath))
        } else {
          setError(msg)
        }
      }).finally(() => setConnectingServerId(null))
    }

    let password: string | undefined
    let keyPath: string | undefined
    
    // Only use stored credentials if remember_me is true
    if (conn.remember_me) {
      if (conn.auth_type === 'password' && !conn.password) {
        // No stored password - show prompt dialog
        showAuthDialog(conn.host, conn.port, conn.username, conn.id, (params) => doConnect(params.username, params.password, params.keyPath))
        return
      }
      if (conn.auth_type === 'password') password = conn.password
      if (conn.auth_type === 'key') keyPath = conn.key_path
    } else {
      // If remember_me is false, always show auth dialog
      showAuthDialog(conn.host, conn.port, conn.username, conn.id, (params) => doConnect(params.username, params.password, params.keyPath))
      return
    }

    doConnect(conn.username, password, keyPath)
  }

  return (
    <div className="app">
      {sidebarVisible && (
        <>
          <div style={{ width: sidebarWidth, minWidth: sidebarWidth, flexShrink: 0, display: 'flex', position: 'relative' }}>
            <Sidebar onSelect={handleSelectConnection} onConnect={handleDirectConnect} onNew={() => clearSession()} onCreateConnection={handleCreateConnection} refreshKey={sidebarRefreshKey} currentSessionId={connectedConfigId} connectingServerId={connectingServerId} />
            {/* Sidebar Toggle Button */}
            <button 
              className="sidebar-toggle-btn visible"
              onClick={() => setSidebarVisible(false)}
              title="Hide Servers Panel"
            >
              HIDE
            </button>
          </div>
          <div
            className="v-divider"
            onMouseDown={() => startDrag('sidebar')}
          />
        </>
      )}
      {!sidebarVisible && (
        <button 
          className="sidebar-toggle-btn hidden"
          onClick={() => setSidebarVisible(true)}
          title="Show Servers Panel"
        >
          SHOW
        </button>
      )}
      <div className="main-area">
        <div className="top-bar">
          {error && <div className="error-bar">{error}</div>}
          {toast && <div className="toast-bar">{toast}</div>}
        </div>
        <div className="split-container" ref={splitContainerRef}>
          <div className="split-full">
            <ServerPanel sessionId={sessionId} connHost={connHost} jumpToPath={jumpToPath} setJumpToPath={setJumpToPath} termRef={termRef} onStartUpload={handleStartUpload} onUploadComplete={uploadCompleteRef} appSettings={settings} onToggleAutoReconnect={toggleAutoReconnect} onUpdateSettings={handleUpdateSettings} onReconnect={handleDisconnectAfterInstall} />
          </div>
        </div>
      </div>
      {/* Password Prompt Dialog */}
      {passwordPrompt.show && (
        <PasswordPromptDialog
          host={passwordPrompt.host}
          username={passwordPrompt.username}
          configId={passwordPrompt.configId}
          onSubmit={(params) => passwordPrompt.retryFn?.(params)}
          onCancel={() => setPasswordPrompt(p => ({ ...p, show: false, retryFn: null }))}
        />
      )}

      {/* Floating Upload Panel */}
      {upload.queue.length > 0 && (
        <UploadPanel
          upload={upload}
          onPause={handlePauseUpload}
          onResume={handleResumeUpload}
          onStop={handleStopUpload}
          onDismiss={handleDismissUpload}
        />
      )}

      {/* Welcome Modal */}
      {showWelcome && (
        <div className="welcome-overlay">
          <div className="welcome-modal">
            <button className="welcome-close-btn" onClick={() => setShowWelcome(false)} title="Close">×</button>
            <div className="welcome-icon"></div>
            <h2 className="welcome-title">Welcome to LeePanel</h2>
            <p className="welcome-subtitle">Your powerful SSH server management companion</p>
            <div className="welcome-features">
              <span>✓ Secure Connections</span>
              <span>✓ File Management</span>
              <span>✓ Server Control</span>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function PasswordPromptDialog({ host, username, configId, onSubmit, onCancel }: {
  host: string
  username: string
  configId?: string
  onSubmit: (params: { username: string; password?: string; keyPath?: string }) => void
  onCancel: () => void
}) {
  const [user, setUser] = useState(username)
  const [authType, setAuthType] = useState<'password' | 'key'>('password')
  const [pw, setPw] = useState('')
  const [showPw, setShowPw] = useState(false)
  const [keyPath, setKeyPath] = useState('')
  const [rememberMe, setRememberMe] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const handleBrowseKey = async () => {
    const selected = await open({ multiple: false, directory: false, title: 'Select SSH Key File', filters: [{ name: 'All Files', extensions: ['*'] }] })
    if (selected) setKeyPath(selected as string)
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    // If we have a configId, save the new credentials before connecting
    if (configId) {
      try {
        await invoke('config_save', {
          connection: {
            id: configId,
            name: '', // Name is not updated here
            host: host.split('_')[0], // Extract host from "host_port" format if needed, or just use host
            port: 22, // Default, actual port is handled by caller
            username: user,
            auth_type: authType,
            remember_me: rememberMe,
            key_path: rememberMe && authType === 'key' ? keyPath : undefined,
            password: rememberMe && authType === 'password' ? pw : undefined,
          },
        })
        console.log('Credentials saved:', { configId, rememberMe, authType })
      } catch (err) {
        console.error('Failed to save credentials:', err)
      }
    }

    if (authType === 'password') {
      if (pw) onSubmit({ username: user, password: pw })
    } else {
      if (keyPath) onSubmit({ username: user, keyPath })
    }
  }

  const canSubmit = authType === 'password' ? !!pw : !!keyPath

  return (
    <div className="fb-dialog-overlay">
      <div className="fb-dialog password-prompt-dialog" onClick={(e) => e.stopPropagation()}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <div className="fb-dialog-title">Incorrect password, please re-enter</div>
          <button type="button" onClick={onCancel} style={{ background: 'none', border: 'none', color: '#8b949e', cursor: 'pointer', fontSize: 20, padding: '4px 8px' }}>×</button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="edit-field">
            <label>Server: <span className="edit-hint">{host}</span></label>
          </div>
          <div className="edit-field">
            <label>Username</label>
            <input
              type="text"
              value={user}
              onChange={(e) => setUser(e.target.value)}
              placeholder="root"
              className="create-input"
            />
          </div>
          <div className="edit-field">
            <label>Auth</label>
            <select className="create-select" value={authType} onChange={(e) => setAuthType(e.target.value as 'password' | 'key')}>
              <option value="password">Password</option>
              <option value="key">Key File</option>
            </select>
          </div>
          {authType === 'password' ? (
            <div className="edit-field">
              <label>Password</label>
              <div className="input-with-toggle">
                <input
                  ref={inputRef}
                  type={showPw ? 'text' : 'password'}
                  value={pw}
                  onChange={(e) => setPw(e.target.value)}
                  placeholder="Enter password"
                  className="create-input"
                />
                <button type="button" className="btn-toggle-password" onClick={() => setShowPw(!showPw)} tabIndex={-1}>
                  {showPw ? '🙈' : '👁'}
                </button>
              </div>
            </div>
          ) : (
            <div className="edit-field">
              <label>Key Path</label>
              <div className="input-with-browse">
                <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} placeholder="~/.ssh/id_rsa" className="create-input" />
                <button type="button" className="btn-browse" onClick={handleBrowseKey} tabIndex={-1}>📂</button>
              </div>
            </div>
          )}
          <div className="fb-dialog-actions">
            <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', marginRight: 'auto' }}>
              <input type="checkbox" checked={rememberMe} onChange={(e) => setRememberMe(e.target.checked)} />
              <span style={{ color: 'red' }}>Remember me</span>
            </label>
            <button type="button" className="fb-dialog-btn" onClick={onCancel}>Cancel</button>
            <button type="submit" className="fb-dialog-btn primary" disabled={!canSubmit}>Connect</button>
          </div>
        </form>
      </div>
    </div>
  )
}

function UploadPanel({ upload, onPause, onResume, onStop, onDismiss }: {
  upload: UploadState
  onPause: () => void
  onResume: () => void
  onStop: () => void
  onDismiss: () => void
}) {
  const [collapsed, setCollapsed] = useState(false)
  const pct = upload.totalBytes > 0 ? Math.round((upload.uploadedBytes / upload.totalBytes) * 100) : 0
  const uploadedMB = (upload.uploadedBytes / 1048576).toFixed(1)
  const totalMB = (upload.totalBytes / 1048576).toFixed(1)
  const remainingMB = ((upload.totalBytes - upload.uploadedBytes) / 1048576).toFixed(1)
  const speedStr = upload.speed >= 1048576
    ? `${(upload.speed / 1048576).toFixed(1)} MB/s`
    : `${(upload.speed / 1024).toFixed(0)} KB/s`
  const doneCount = upload.queue.filter(q => q.status === 'done').length
  const allDone = !upload.active && upload.queue.every(q => q.status === 'done' || q.status === 'error' || q.status === 'stopped')

  return (
    <div className={`upload-panel ${collapsed ? 'collapsed' : ''}`}>
      <div className="upload-panel-header" onClick={() => setCollapsed(!collapsed)}>
        <span className="upload-panel-title">
          📤 {upload.active ? (upload.paused ? 'Paused' : 'Uploading') : allDone ? 'Complete' : 'Stopped'}
          {' '}{doneCount}/{upload.queue.length}
          {upload.active && !upload.paused && ` — ${pct}%`}
        </span>
        <span className="upload-panel-toggle">{collapsed ? '▲' : '▼'}</span>
      </div>
      {!collapsed && (
        <>
          {upload.active && (
            <div className="upload-panel-progress">
              <div className="upload-progress-track">
                <div className="upload-progress-fill" style={{ width: `${pct}%` }} />
              </div>
              <div className="upload-progress-info">
                {uploadedMB}M / {totalMB}M | Remaining {remainingMB}M | {speedStr}
              </div>
            </div>
          )}
          <div className="upload-panel-queue">
            {upload.queue.map((item, i) => (
              <div key={i} className={`upload-queue-item ${item.status}`}>
                <span className="upload-item-icon">
                  {item.status === 'done' ? '✅' : item.status === 'error' ? '❌' : item.status === 'stopped' ? '⏹' : item.status === 'uploading' ? '⬆️' : '⏳'}
                </span>
                <span className="upload-item-name" title={item.fileName}>{item.fileName}</span>
                <span className="upload-item-size">{(item.file.size / 1048576).toFixed(1)}M</span>
                {item.error && <span className="upload-item-error" title={item.error}>!</span>}
              </div>
            ))}
          </div>
          <div className="upload-panel-actions">
            {upload.active && !upload.paused && (
              <>
                <button className="upload-btn" onClick={onPause} title="Pause">⏸ Pause</button>
                <button className="upload-btn danger" onClick={onStop} title="Stop">⏹ Stop</button>
              </>
            )}
            {upload.active && upload.paused && (
              <>
                <button className="upload-btn" onClick={onResume} title="Resume">▶ Resume</button>
                <button className="upload-btn danger" onClick={onStop} title="Stop">⏹ Stop</button>
              </>
            )}
            {!upload.active && (
              <button className="upload-btn" onClick={onDismiss} title="Close">✕ Close</button>
            )}
          </div>
        </>
      )}
    </div>
  )
}

export default App
