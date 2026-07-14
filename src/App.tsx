import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { check } from '@tauri-apps/plugin-updater'
import { useTranslation } from 'react-i18next'
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

interface Settings {
  auto_reconnect: boolean
  reconnect_interval: number
  max_reconnect_attempts: number
  cache_ttl_hours: number
  cache_max_files: number
  cache_enabled: boolean
  command_timeout_minutes: number
}

function App() {
  const { t } = useTranslation()
  const [sessionId, setSessionId] = useState<string | null>(null)
  const [connectedConfigId, setConnectedConfigId] = useState<string | null>(null)
  const [connectingServerId, setConnectingServerId] = useState<string | null>(null)
  const [error, setError] = useState('')
  const [toast, setToast] = useState('')
  const [showWelcome, setShowWelcome] = useState(false)
  const termRef = useRef<TerminalHandle | null>(null)
  const [errorDialog, setErrorDialog] = useState<{ visible: boolean; message: string; type: 'auth' | 'network' | 'connection' | 'other' } | null>(null)

  // Settings
  const [settings, setSettings] = useState<Settings>({
    auto_reconnect: true, reconnect_interval: 5, max_reconnect_attempts: 10, cache_ttl_hours: 24, cache_max_files: 500, cache_enabled: true, command_timeout_minutes: 30
  })
  const [, setReconnecting] = useState(false)
  const reconnectAttemptRef = useRef(0)
  const autoReconnectRef = useRef(true)
  const reconnectingRef = useRef(false)
  const manualDisconnectRef = useRef(false)
  const [sidebarRefreshKey, setSidebarRefreshKey] = useState(0)
  const [connHost, setConnHost] = useState('')
  const [connUsername, setConnUsername] = useState('')
  const [initialSection, setInitialSection] = useState<string>('dashboard')

  const clearSession = () => {
    setSessionId(null)
    setConnectedConfigId(null)
    setConnHost('')
    setConnUsername('')
    setInitialSection('dashboard')
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
        const doClear = () => {
          clearSession()
          termRef.current?.clear()
        }
        // ponytail: race disconnect against 3s local timeout — ensures UI always responds
        Promise.race([
          invoke('ssh_disconnect', { sessionId }).catch(() => {}),
          new Promise<void>(resolve => setTimeout(resolve, 3000)),
        ]).then(doClear)
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

    // ponytail: 3 concurrent workers — SFTP session reused via cache, 3x throughput
    const CONCURRENCY = Math.min(3, files.length)
    let nextIndex = 0

    const updateSpeed = () => {
      const elapsed = (Date.now() - startTime) / 1000
      const speed = elapsed > 0 ? uploadedBytes / elapsed : 0
      setUpload(prev => ({ ...prev, uploadedBytes, speed }))
    }

    const worker = async () => {
      while (true) {
        if (uploadStopRef.current) return
        const i = nextIndex++
        if (i >= queue.length) return
        const item = queue[i]

        setUpload(prev => ({
          ...prev,
          queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'uploading' } : q)
        }))

        try {
          let offset = 0
          while (offset < item.file.size) {
            if (uploadStopRef.current) {
              setUpload(prev => ({
                ...prev, active: false, paused: false,
                queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'stopped' } : q)
              }))
              return
            }
            while (uploadPauseRef.current) {
              if (uploadStopRef.current) {
                setUpload(prev => ({
                  ...prev, active: false, paused: false,
                  queue: prev.queue.map((q, j) => j === i ? { ...q, status: 'stopped' } : q)
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
            updateSpeed()
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
    }

    const workers = Array.from({ length: CONCURRENCY }, () => worker())
    await Promise.all(workers)

    if (!uploadStopRef.current) {
      setUpload(prev => ({ ...prev, active: false, paused: false }))
      uploadCompleteRef.current?.()
    }
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
        showToast(t('common.connectionLost'))
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

  const classifyError = (errorMsg: string): { type: 'auth' | 'network' | 'connection' | 'other'; message: string } => {
    const s = errorMsg.toLowerCase()
    
    // Authentication errors
    if (s.includes('auth failed') || s.includes('auth error') || s.includes('authentication') || 
        s.includes('no authentication') || s.includes('permission denied') || s.includes('invalid password')) {
      return { type: 'auth', message: 'Authentication failed. Please check your username and password.' }
    }
    
    // Network errors
    if (s.includes('timeout') || s.includes('timed out') || s.includes('network unreachable')) {
      return { type: 'network', message: 'Connection timed out. Please check network connectivity.' }
    }
    
    // Connection refused
    if (s.includes('connection refused') || s.includes('host unreachable')) {
      return { type: 'connection', message: 'Connection refused. Server may be offline or port is incorrect.' }
    }
    
    // Key file errors
    if (s.includes('key') && (s.includes('not found') || s.includes('invalid'))) {
      return { type: 'auth', message: 'SSH key file not found or invalid.' }
    }
    
    // Default
    return { type: 'other', message: errorMsg }
  }

  const handleSelectConnection = (_conn: SidebarConnection) => {
    // ponytail: no-op, kept for interface compatibility
  }

  const handleDirectConnect = useCallback(async (conn: SidebarConnection) => {
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
      const hostKey = `${conn.host}_${conn.port}`
      const panelKey = `lastPanel_${username}@${hostKey}`
      // ponytail: parallel SSH + DB read → no flash, correct page rendered immediately
      Promise.all([
        Promise.race([
          invoke<string>('ssh_connect', {
            config: { host: conn.host, port: conn.port, username, password, keyPath },
          }),
          new Promise<never>((_, reject) => setTimeout(() => reject(new Error('Connection timeout')), 20000)),
        ]),
        invoke<string>('ui_state_get', { key: panelKey }).catch(() => ''),
      ]).then(([sid, savedPanel]) => {
        setSessionId(sid)
        setConnectedConfigId(conn.id)
        console.log('Direct connect! sessionId:', sid, 'configId:', conn.id)
        setConnHost(hostKey)
        setConnUsername(username)
        setInitialSection(savedPanel || 'dashboard')
        manualDisconnectRef.current = false
        // Show welcome modal on successful connection (once per 6 hours)
        const WELCOME_INTERVAL = 6 * 60 * 60 * 1000
        const lastShown = Number(localStorage.getItem('welcome_last_shown') || 0)
        if (Date.now() - lastShown >= WELCOME_INTERVAL) {
          setShowWelcome(true)
          localStorage.setItem('welcome_last_shown', String(Date.now()))
          console.log('Showing welcome modal (direct connect)')
          setTimeout(() => {
            setShowWelcome(false)
            console.log('Hiding welcome modal after 4 seconds')
          }, 4000)
        } else {
          console.log('Welcome modal skipped, shown recently')
        }
      }).catch(e => {
        const msg = String(e)
        const { type, message } = classifyError(msg)
        setErrorDialog({ visible: true, message, type })
      }).finally(() => setConnectingServerId(null))
    }

    let password: string | undefined
    let keyPath: string | undefined
    
    // Only use stored credentials if remember_me is true
    if (conn.remember_me) {
      if (conn.auth_type === 'password' && !conn.password) {
        setErrorDialog({ visible: true, message: 'No password saved. Please edit the connection to add credentials.', type: 'auth' })
        return
      }
      if (conn.auth_type === 'password') password = conn.password
      if (conn.auth_type === 'key') keyPath = conn.key_path
    } else {
      setErrorDialog({ visible: true, message: 'Please edit the connection to configure authentication.', type: 'auth' })
      return
    }

    doConnect(conn.username, password, keyPath)
  }, [sessionId])

  // Listen for reconnect-after-edit from Sidebar (Connect button)
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail?.conn) handleDirectConnect(detail.conn)
    }
    window.addEventListener('sidebar-reconnect-after-edit', handler)
    return () => window.removeEventListener('sidebar-reconnect-after-edit', handler)
  }, [handleDirectConnect])

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
              title={t('common.hidePanel')}
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
          title={t('common.showPanel')}
        >
          SHOW
        </button>
      )}
      <div className="main-area">
        <div className="top-bar">
          {error && <div className="error-bar">{error}</div>}
          {toast && <div className="toast-bar">{toast}</div>}
        </div>
        
        {/* Error Dialog */}
        {errorDialog?.visible && (
          <div className="error-dialog-overlay" onClick={() => setErrorDialog(null)}>
            <div className="error-dialog" onClick={(e) => e.stopPropagation()}>
              <button className="error-dialog-close" onClick={() => setErrorDialog(null)}>×</button>
              <div className="error-dialog-icon">
                {errorDialog.type === 'auth' && '🔐'}
                {errorDialog.type === 'network' && '🌐'}
                {errorDialog.type === 'connection' && '⚠️'}
                {errorDialog.type === 'other' && '❗'}
              </div>
              <div className="error-dialog-title">{t('errorDialog.connectionFailed')}</div>
              <div className="error-dialog-message">{errorDialog.message}</div>
              <button className="error-dialog-btn" onClick={() => setErrorDialog(null)}>{t('common.close')}</button>
            </div>
          </div>
        )}
        <div className="split-container" ref={splitContainerRef}>
          <div className="split-full">
            <ServerPanel key={connHost} sessionId={sessionId} connHost={connHost} connUsername={connUsername} initialSection={initialSection} jumpToPath={jumpToPath} setJumpToPath={setJumpToPath} termRef={termRef} onStartUpload={handleStartUpload} onUploadComplete={uploadCompleteRef} appSettings={settings} onToggleAutoReconnect={toggleAutoReconnect} onUpdateSettings={handleUpdateSettings} />
          </div>
        </div>
      </div>


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
            <button className="welcome-close-btn" onClick={() => setShowWelcome(false)} title={t('common.close')}>×</button>
            <div className="welcome-icon"></div>
            <h2 className="welcome-title">{t('welcome.title')}</h2>
            <p className="welcome-subtitle">{t('welcome.subtitle')}</p>
            <div className="welcome-features">
              <span>✓ {t('welcome.secureConnections')}</span>
              <span>✓ {t('welcome.fileManagement')}</span>
              <span>✓ {t('welcome.serverControl')}</span>
            </div>
          </div>
        </div>
      )}
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
  const { t } = useTranslation()
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
          📤 {upload.active ? (upload.paused ? t('upload.paused') : t('upload.uploading')) : allDone ? t('upload.complete') : t('upload.stopped')}
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
                {uploadedMB}M / {totalMB}M | {t('upload.remaining')} {remainingMB}M | {speedStr}
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
                <button className="upload-btn" onClick={onPause} title={t('upload.pause')}>⏸ {t('upload.pause')}</button>
                <button className="upload-btn danger" onClick={onStop} title={t('upload.stop')}>⏹ {t('upload.stop')}</button>
              </>
            )}
            {upload.active && upload.paused && (
              <>
                <button className="upload-btn" onClick={onResume} title={t('upload.resume')}>▶ {t('upload.resume')}</button>
                <button className="upload-btn danger" onClick={onStop} title={t('upload.stop')}>⏹ {t('upload.stop')}</button>
              </>
            )}
            {!upload.active && (
              <button className="upload-btn" onClick={onDismiss} title={t('common.close')}>✕ {t('common.close')}</button>
            )}
          </div>
        </>
      )}
    </div>
  )
}

export default App
