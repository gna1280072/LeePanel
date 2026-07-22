import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { check, type Update } from '@tauri-apps/plugin-updater'
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
  retryCount?: number
}

interface UploadState {
  queue: UploadItem[]
  totalBytes: number
  uploadedBytes: number
  speed: number
  active: boolean
  paused: boolean
  workers: number
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
  upload_workers: number
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
  const [pendingUpdate, setPendingUpdate] = useState<Update | null>(null)

  // Settings
  const [settings, setSettings] = useState<Settings>({
    auto_reconnect: true, reconnect_interval: 5, max_reconnect_attempts: 10, cache_ttl_hours: 24, cache_max_files: 500, cache_enabled: true, command_timeout_minutes: 30, upload_workers: 3
  })
  const [reconnecting, setReconnecting] = useState(false)
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
    queue: [], totalBytes: 0, uploadedBytes: 0, speed: 0, active: false, paused: false, workers: 0
  })
  const uploadPauseRef = useRef(false)
  const uploadStopRef = useRef(false)
  const uploadCompleteRef = useRef<(() => void) | null>(null)

  // ponytail: build a POSIX tar archive in the browser — no dependencies
  const createTar = async (entries: { name: string; file: File }[]): Promise<Uint8Array> => {
    const chunks: Uint8Array[] = []
    for (const { name, file } of entries) {
      const data = new Uint8Array(await file.arrayBuffer())
      const header = new Uint8Array(512)
      const enc = new TextEncoder()
      header.set(enc.encode(name), 0)
      header.set(enc.encode('0000644\0'), 100)  // mode
      header.set(enc.encode('0001000\0'), 108)  // uid
      header.set(enc.encode('0001000\0'), 116)  // gid
      header.set(enc.encode(file.size.toString(8).padStart(11, '0') + '\0'), 124)
      header.set(enc.encode(Math.floor(Date.now() / 1000).toString(8).padStart(11, '0') + '\0'), 136)
      header.set(enc.encode('        '), 148) // checksum placeholder (spaces)
      header[156] = 0x30 // type '0' = regular file
      header.set(enc.encode('ustar\0'), 257)
      header.set(enc.encode('00'), 263)
      // compute checksum
      let cksum = 0
      for (let i = 0; i < 512; i++) cksum += header[i]
      header.set(enc.encode(cksum.toString(8).padStart(6, '0') + '\0 '), 148)
      chunks.push(header)
      chunks.push(data)
      const padLen = (512 - (data.length % 512)) % 512
      if (padLen > 0) chunks.push(new Uint8Array(padLen))
    }
    chunks.push(new Uint8Array(1024)) // terminator
    const total = chunks.reduce((s, c) => s + c.length, 0)
    const result = new Uint8Array(total)
    let off = 0
    for (const c of chunks) { result.set(c, off); off += c.length }
    return result
  }

  const handleStartUpload = useCallback(async (files: { file: File; fileName: string; remotePath: string }[]) => {
    if (!sessionId || files.length === 0) return
    const sid = sessionId
    const totalBytes = files.reduce((sum, f) => sum + f.file.size, 0)
    const retryCounts = new Map<string, number>()
    const queue: UploadItem[] = files.map(f => ({ ...f, status: 'pending' as const, retryCount: 0 }))
    setUpload({ queue, totalBytes, uploadedBytes: 0, speed: 0, active: true, paused: false, workers: 0 })
    uploadPauseRef.current = false
    uploadStopRef.current = false

    let uploadedBytes = 0
    let activeWorkers = 0
    const startTime = Date.now()
    const CHUNK_SIZE = 1024 * 1024
    // ponytail: files < 1MB go to tar batch; large files use chunked workers
    const SMALL_THRESHOLD = CHUNK_SIZE

    const updateSpeed = () => {
      const elapsed = (Date.now() - startTime) / 1000
      const speed = elapsed > 0 ? uploadedBytes / elapsed : 0
      setUpload(prev => ({ ...prev, uploadedBytes, speed, workers: activeWorkers }))
    }

    // ponytail: batch small files by parent directory into tar archives — N SFTP ops → 1 per dir
    const smallFiles: typeof files = []
    const largeFiles: typeof files = []
    for (const f of files) {
      if (f.file.size < SMALL_THRESHOLD && f.file.size > 0) smallFiles.push(f)
      else largeFiles.push(f)
    }

    if (smallFiles.length > 1) {
      // group by parent directory
      const byDir = new Map<string, typeof files>()
      for (const f of smallFiles) {
        const parent = f.remotePath.substring(0, f.remotePath.lastIndexOf('/'))
        if (!byDir.has(parent)) byDir.set(parent, [])
        byDir.get(parent)!.push(f)
      }

      // process directories with concurrency of 3
      const dirEntries = [...byDir.entries()]
      let dirIdx = 0
      const batchWorker = async () => {
        activeWorkers++
        updateSpeed()
        try {
        while (dirIdx < dirEntries.length) {
          if (uploadStopRef.current) return
          const i = dirIdx++
          const [parentDir, dirFiles] = dirEntries[i]

          // mark batch as uploading
          const indices = dirFiles.map(df => queue.indexOf(queue.find(q => q.remotePath === df.remotePath)!))
          setUpload(prev => ({
            ...prev,
            queue: prev.queue.map((q, j) => indices.includes(j) ? { ...q, status: 'uploading' } : q)
          }))

          try {
            const tarEntries = dirFiles.map(f => ({
              name: f.fileName.split('/').pop()!, // just filename, extract in target dir
              file: f.file,
            }))
            const tarData = await createTar(tarEntries)
            const tarPath = `${parentDir}/.__tb_${Date.now()}_${i}.tar`

            // upload tar in chunks
            let offset = 0
            while (offset < tarData.length) {
              if (uploadStopRef.current) return
              const end = Math.min(offset + CHUNK_SIZE, tarData.length)
              const chunk = tarData.slice(offset, end)
              await invoke('ssh_upload_chunk', {
                sessionId: sid, remotePath: tarPath, data: chunk, offset,
              })
              uploadedBytes += (end - offset)
              offset = end
              updateSpeed()
            }

            // extract + cleanup
            const escaped = (s: string) => s.replace(/'/g, "'\\''")
            const cmd = `cd '${escaped(parentDir)}' && tar xf '${escaped(tarPath.split('/').pop()!)}' && rm -f '${escaped(tarPath.split('/').pop()!)}'`
            const result = await invoke<[string, string, number]>('ssh_exec', { sessionId: sid, command: cmd })
            if (result[2] !== 0) throw new Error(`tar extract failed: ${result[1]}`)

            setUpload(prev => ({
              ...prev,
              queue: prev.queue.map((q, j) => indices.includes(j) ? { ...q, status: 'done' } : q)
            }))
          } catch (err) {
            if (uploadStopRef.current) return
            // ponytail: auto-retry up to 3 times per file before marking error
            const canRetry = dirFiles.every(f => (retryCounts.get(f.remotePath) || 0) < 3)
            if (canRetry) {
              dirFiles.forEach(f => retryCounts.set(f.remotePath, (retryCounts.get(f.remotePath) || 0) + 1))
              await invoke('ssh_sftp_reset', { sessionId: sid }).catch(() => {})
              await new Promise(r => setTimeout(r, 1000))
              if (uploadStopRef.current) return
              setUpload(prev => ({
                ...prev,
                queue: prev.queue.map((q, j) => indices.includes(j) ? { ...q, status: 'pending' as const, retryCount: retryCounts.get(q.remotePath) || 0 } : q)
              }))
              dirEntries.push([parentDir, dirFiles])
            } else {
              setUpload(prev => ({
                ...prev,
                queue: prev.queue.map((q, j) => indices.includes(j) ? { ...q, status: 'error', error: String(err), retryCount: retryCounts.get(q.remotePath) || 0 } : q)
              }))
            }
          }
        }
        } finally { activeWorkers--; updateSpeed() }
      }
      const batchWorkers = Array.from({ length: Math.min(settings.upload_workers || 3, dirEntries.length) }, () => batchWorker())
      await Promise.all(batchWorkers)
    }

    if (uploadStopRef.current) return

    // ponytail: large files + zero-byte files via chunked workers
    const largeQueue: UploadItem[] = largeFiles.map(f => ({ ...f, status: 'pending' as const }))
    if (largeQueue.length > 0) {
      // update main queue to reflect only large files remaining
      setUpload(prev => ({
        ...prev,
        queue: prev.queue.map(q => {
          const inLarge = largeFiles.some(lf => lf.remotePath === q.remotePath)
          return inLarge ? { ...q, status: 'pending' as const } : q
        })
      }))

      const CONCURRENCY = Math.min(settings.upload_workers || 3, largeQueue.length)
      let nextIndex = 0

      const worker = async () => {
        activeWorkers++
        updateSpeed()
        try {
        while (true) {
          if (uploadStopRef.current) return
          const i = nextIndex++
          if (i >= largeQueue.length) return
          const item = largeQueue[i]

          setUpload(prev => ({
            ...prev,
            queue: prev.queue.map(q => q.remotePath === item.remotePath ? { ...q, status: 'uploading' } : q)
          }))

          try {
            let offset = 0
            while (offset < item.file.size) {
              if (uploadStopRef.current) return
              while (uploadPauseRef.current) {
                if (uploadStopRef.current) return
                await new Promise(r => setTimeout(r, 100))
              }

              const end = Math.min(offset + CHUNK_SIZE, item.file.size)
              const slice = item.file.slice(offset, end)
              const buffer = await slice.arrayBuffer()
              const chunkData = new Uint8Array(buffer)
              try {
                await invoke('ssh_upload_chunk', {
                  sessionId: sid,
                  remotePath: item.remotePath,
                  data: chunkData,
                  offset,
                })
              } catch (_chunkErr) {
                if (uploadStopRef.current) return
                await invoke('ssh_sftp_reset', { sessionId: sid }).catch(() => {})
                await new Promise(r => setTimeout(r, 500))
                if (uploadStopRef.current) return
                await invoke('ssh_upload_chunk', {
                  sessionId: sid,
                  remotePath: item.remotePath,
                  data: chunkData,
                  offset,
                })
              }
              uploadedBytes += (end - offset)
              offset = end
              updateSpeed()
            }
            setUpload(prev => ({
              ...prev,
              queue: prev.queue.map(q => q.remotePath === item.remotePath ? { ...q, status: 'done' } : q)
            }))
          } catch (err) {
            if (uploadStopRef.current) return
            // ponytail: auto-retry up to 3 times before marking error
            const count = (retryCounts.get(item.remotePath) || 0) + 1
            retryCounts.set(item.remotePath, count)
            if (count < 3) {
              await invoke('ssh_sftp_reset', { sessionId: sid }).catch(() => {})
              await new Promise(r => setTimeout(r, 1000))
              if (uploadStopRef.current) return
              setUpload(prev => ({
                ...prev,
                queue: prev.queue.map(q => q.remotePath === item.remotePath ? { ...q, status: 'pending' as const, retryCount: count } : q)
              }))
              largeQueue.push(item)
            } else {
              setUpload(prev => ({
                ...prev,
                queue: prev.queue.map(q => q.remotePath === item.remotePath ? { ...q, status: 'error', error: String(err), retryCount: count } : q)
              }))
            }
          }
        }
        } finally { activeWorkers--; updateSpeed() }
      }

      const workers = Array.from({ length: CONCURRENCY }, () => worker())
      await Promise.all(workers)
    }

    if (!uploadStopRef.current) {
      setUpload(prev => ({ ...prev, active: false, paused: false }))
      uploadCompleteRef.current?.()
    }
  }, [sessionId, settings.upload_workers])

  const handlePauseUpload = useCallback(() => {
    uploadPauseRef.current = true
    setUpload(prev => ({ ...prev, paused: true }))
  }, [])

  const handleResumeUpload = useCallback(() => {
    uploadPauseRef.current = false
    setUpload(prev => ({ ...prev, paused: false }))
  }, [])

  // ponytail: stop = immediately clear UI + signal workers to exit silently
  const handleStopUpload = useCallback(() => {
    uploadStopRef.current = true
    uploadPauseRef.current = false
    setUpload({ queue: [], totalBytes: 0, uploadedBytes: 0, speed: 0, active: false, paused: false, workers: 0 })
  }, [])

  const handleDismissUpload = useCallback(() => {
    if (upload.active) return
    setUpload({ queue: [], totalBytes: 0, uploadedBytes: 0, speed: 0, active: false, paused: false, workers: 0 })
  }, [upload.active])

  // ponytail: retry only failed files — re-queues them through the same upload pipeline
  const handleRetryFailed = useCallback(() => {
    const failed = upload.queue.filter(q => q.status === 'error')
    if (failed.length === 0) return
    handleStartUpload(failed.map(f => ({ file: f.file, fileName: f.fileName, remotePath: f.remotePath })))
    // handleStartUpload creates a fresh retryCounts map, so retries reset to 0
  }, [upload.queue, handleStartUpload])

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
    // ponytail: auto-check for updates on startup, ask user before downloading
    Promise.race([
      check(),
      new Promise<never>((_, reject) => setTimeout(() => reject(new Error('Timeout')), 15000)),
    ]).then(async update => {
      if (update?.available) {
        const { ask } = await import('@tauri-apps/plugin-dialog')
        const yes = await ask(`New version ${update.version} available. Update now?`, { title: 'Update Available', kind: 'info' })
        if (yes) {
          showToast(`Downloading v${update.version}...`)
          try {
            await update.download()
            const restart = await ask(`v${update.version} has been downloaded. Restart now to apply the update?`, { title: 'Update Ready', kind: 'info' })
            if (restart) {
              await update.install()
            } else {
              setPendingUpdate(update)
              showToast('Update ready. Click "Restart Now" when you are ready.')
            }
          } catch (e) {
            showToast(`Update failed: ${String(e).slice(0, 80)}`)
          }
        }
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

  const handleStopReconnect = () => {
    reconnectingRef.current = false
    setReconnecting(false)
    clearSession()
  }

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
          {toast && (
            <div className="toast-bar">
              <span>{toast}</span>
              {reconnecting && (
                <button className="toast-stop-btn" onClick={() => { reconnectingRef.current = false; setReconnecting(false); clearSession() }}>Stop</button>
              )}
            </div>
          )}
          {pendingUpdate && (
            <div className="update-ready-bar">
              <span>🔄 Update v{pendingUpdate.version} ready</span>
              <button className="update-restart-btn" onClick={async () => { await pendingUpdate.install() }}>Restart Now</button>
            </div>
          )}
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
                    onRetry={handleRetryFailed}
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



function UploadPanel({ upload, onPause, onResume, onStop, onDismiss, onRetry }: {
  upload: UploadState
  onPause: () => void
  onResume: () => void
  onStop: () => void
  onDismiss: () => void
    onRetry: () => void
}) {
  const { t } = useTranslation()
  const [collapsed, setCollapsed] = useState(false)
  const [showStopConfirm, setShowStopConfirm] = useState(false)
  const [stopInput, setStopInput] = useState('')
  const stopInputRef = useRef<HTMLInputElement>(null)
  const stopConfirmed = stopInput.trim().toLowerCase() === 'stop'
  const pct = upload.totalBytes > 0 ? Math.round((upload.uploadedBytes / upload.totalBytes) * 100) : 0
  const uploadedMB = (upload.uploadedBytes / 1048576).toFixed(1)
  const totalMB = (upload.totalBytes / 1048576).toFixed(1)
  const remainingMB = ((upload.totalBytes - upload.uploadedBytes) / 1048576).toFixed(1)
  const speedStr = upload.speed >= 1048576
    ? `${(upload.speed / 1048576).toFixed(1)} MB/s`
    : `${(upload.speed / 1024).toFixed(0)} KB/s`
  const doneCount = upload.queue.filter(q => q.status === 'done').length
  const allDone = !upload.active && upload.queue.every(q => q.status === 'done' || q.status === 'error' || q.status === 'stopped')
    const failedCount = upload.queue.filter(q => q.status === 'error').length

  return (
    <div className={`upload-panel ${collapsed ? 'collapsed' : ''}`}>
      <div className="upload-panel-header" onClick={() => setCollapsed(!collapsed)}>
        <span className="upload-panel-title">
          📤 {upload.active ? (upload.paused ? t('upload.paused') : t('upload.uploading')) : allDone ? t('upload.complete') : t('upload.stopped')}
          {' '}{doneCount}/{upload.queue.length}
          {upload.active && !upload.paused && ` — ${pct}% — 👷 ${upload.workers}`}
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
                {item.retryCount && item.retryCount > 0 && item.status === 'pending' && <span className="upload-item-retry">🔄 {item.retryCount}/3</span>}
                {item.error && <span className="upload-item-error" title={item.error}>!</span>}
              </div>
            ))}
          </div>
          <div className="upload-panel-actions">
            {upload.active && !upload.paused && (
              <>
                <button className="upload-btn" onClick={onPause} title={t('upload.pause')}>⏸ {t('upload.pause')}</button>
                <button className="upload-btn" onClick={() => { setShowStopConfirm(true); setStopInput('') }} title={t('upload.stop')}>⏹ {t('upload.stop')}</button>
              </>
            )}
            {upload.active && upload.paused && (
              <>
                <button className="upload-btn" onClick={onResume} title={t('upload.resume')}>▶ {t('upload.resume')}</button>
                <button className="upload-btn" onClick={() => { setShowStopConfirm(true); setStopInput('') }} title={t('upload.stop')}>⏹ {t('upload.stop')}</button>
              </>
            )}
            {!upload.active && (
              <>
                {failedCount > 0 && (
                  <button className="upload-btn" onClick={onRetry} title={t('upload.retryFailed')}>🔄 {t('upload.retryFailed')} ({failedCount})</button>
                )}
                <button className="upload-btn" onClick={onDismiss} title={t('common.close')}>✕ {t('common.close')}</button>
              </>
            )}
          </div>
        </>
      )}

      {/* Stop confirmation modal */}
      {showStopConfirm && (
        <div className="fb-dialog-overlay" onClick={() => setShowStopConfirm(false)}>
          <div className="fb-dialog" onClick={(e) => e.stopPropagation()} style={{ minWidth: 380 }}>
            <button className="modal-close-btn" onClick={() => setShowStopConfirm(false)} title={t('common.close')}>×</button>
            <div className="fb-dialog-title" style={{ marginBottom: 12 }}>{t('upload.confirmStopTitle')}</div>
            <p style={{ margin: '0 0 16px', fontSize: 13, color: '#8b949e', lineHeight: 1.6 }}>
              {t('upload.confirmStopMsg')}
            </p>
            <input
              ref={stopInputRef}
              className="fb-dialog-input"
              value={stopInput}
              onChange={e => setStopInput(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && stopConfirmed) { onStop(); setShowStopConfirm(false) } }}
              placeholder={t('upload.confirmStopPlaceholder')}
              autoFocus
              style={{ width: '100%', padding: '8px 12px', borderRadius: 6, border: '1px solid #30363d', background: '#0d1117', color: '#c9d1d9', fontSize: 13, outline: 'none', boxSizing: 'border-box' }}
            />
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn" onClick={() => setShowStopConfirm(false)}>{t('common.cancel')}</button>
              <button className="fb-dialog-btn danger" disabled={!stopConfirmed} onClick={() => { onStop(); setShowStopConfirm(false) }} style={{ opacity: stopConfirmed ? 1 : 0.4 }}>
                {t('upload.stop')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

export default App
