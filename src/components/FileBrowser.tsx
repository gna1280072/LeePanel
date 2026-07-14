import { useState, useEffect, useCallback, useRef, useMemo, useImperativeHandle, forwardRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { readFile, readDir } from '@tauri-apps/plugin-fs'
import { useTranslation } from 'react-i18next'

interface FileEntry {
  name: string
  isDir: boolean
  isSymlink: boolean
  size: number
  permissions: string
  mtime: number
  owner: string
}

interface FileBrowserProps {
  sessionId: string | null
  connHost?: string
  jumpToPath?: string | null
  onTerminalCommand?: (cmd: string) => void
  onCdHere?: (path: string) => void
  onStartUpload?: (files: { file: File; fileName: string; remotePath: string }[]) => void
}

export interface FileBrowserHandle {
  jumpToPath: (path: string) => void
  refreshCurrentDirectory: () => void
}

interface FileContextMenu {
  x: number
  y: number
  entry: FileEntry
}

interface EditorState {
  path: string
  name: string
  content: string
  originalContent: string
  saving: boolean
  maximized?: boolean
  minimized?: boolean
}

interface Clipboard {
  paths: string[]
  names: string[]
  isDirs: boolean[]
  hasDirs: boolean
  mode: 'copy' | 'cut'
}

interface Toast {
  message: string
  type: 'success' | 'error' | 'info'
}

interface FileInfo {
  entry: FileEntry
  path: string
}

interface ConflictItem {
  name: string
  isDir: boolean
}

interface ConflictDialog {
  item: ConflictItem
  remaining: number
  resolve: (result: { action: 'replace' | 'rename' | 'skip', applyToAll: boolean }) => void
}

interface ConfirmDialog {
  message: string
  onConfirm: () => void
}

function formatSize(bytes: number): string {
  if (bytes === 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  const val = bytes / Math.pow(1024, i)
  return val >= 100 ? `${Math.round(val)} ${units[i]}` : `${val.toFixed(1)} ${units[i]}`
}

function formatTime(unix: number): string {
  if (unix === 0) return '—'
  const d = new Date(unix * 1000)
  const pad = (n: number) => n.toString().padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function getFileExtension(name: string): string {
  const i = name.lastIndexOf('.')
  return i > 0 ? name.substring(i + 1).toLowerCase() : ''
}

const TEXT_EXTS = new Set([
  'txt', 'md', 'log', 'json', 'xml', 'yml', 'yaml', 'toml', 'ini', 'cfg', 'conf',
  'js', 'ts', 'jsx', 'tsx', 'py', 'rb', 'go', 'rs', 'java', 'c', 'cpp', 'h', 'hpp',
  'css', 'html', 'htm', 'scss', 'less', 'vue', 'svelte',
  'sh', 'bash', 'zsh', 'fish', 'ps1', 'bat', 'cmd',
  'sql', 'graphql', 'proto', 'dockerfile', 'makefile',
  'env', 'gitignore', 'editorconfig', 'htaccess',
  'php', 'lua', 'pl', 'swift', 'kt', 'scala', 'r', 'm',
])

function isTextFile(name: string): boolean {
  const ext = getFileExtension(name)
  return TEXT_EXTS.has(ext) || name.startsWith('.') || !name.includes('.')
}

const IMAGE_EXTS = new Set([
  'png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg', 'ico', 'tiff', 'tif',
])

function isImageFile(name: string): boolean {
  return IMAGE_EXTS.has(getFileExtension(name))
}

// SVG Icons
const FolderIcon = () => (
  <svg width="48" height="48" viewBox="0 0 48 48" fill="none">
    <path d="M4 10C4 8.89543 4.89543 8 6 8H18L22 12H42C43.1046 12 44 12.8954 44 14V38C44 39.1046 43.1046 40 42 40H6C4.89543 40 4 39.1046 4 38V10Z" fill="#E8A838"/>
    <path d="M4 14H44V38C44 39.1046 43.1046 40 42 40H6C4.89543 40 4 39.1046 4 38V14Z" fill="#F5C451"/>
  </svg>
)

const FileIcon = () => (
  <svg width="48" height="48" viewBox="0 0 48 48" fill="none">
    <path d="M10 4H30L40 14V42C40 43.1046 39.1046 44 38 44H10C8.89543 44 8 43.1046 8 42V6C8 4.89543 8.89543 4 10 4Z" fill="#3B4252"/>
    <path d="M30 4L40 14H32C30.8954 14 30 13.1046 30 12V4Z" fill="#4C566A"/>
    <path d="M14 22H34M14 28H34M14 34H26" stroke="#8892A8" strokeWidth="1.5" strokeLinecap="round"/>
  </svg>
)

const SymlinkIcon = () => (
  <svg width="48" height="48" viewBox="0 0 48 48" fill="none">
    <path d="M10 4H30L40 14V42C40 43.1046 39.1046 44 38 44H10C8.89543 44 8 43.1046 8 42V6C8 4.89543 8.89543 4 10 4Z" fill="#2E3440"/>
    <path d="M30 4L40 14H32C30.8954 14 30 13.1046 30 12V4Z" fill="#4C566A"/>
    <path d="M20 20L28 24L20 28" stroke="#88C0D0" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
    <path d="M28 24H16" stroke="#88C0D0" strokeWidth="2" strokeLinecap="round"/>
  </svg>
)

const BackIcon = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
    <path d="M7.78 1.22a.75.75 0 0 1 0 1.06L4.56 5.5H12a.75.75 0 0 1 0 1.5H4.56l3.22 3.22a.75.75 0 1 1-1.06 1.06l-4.5-4.5a.75.75 0 0 1 0-1.06l4.5-4.5a.75.75 0 0 1 1.06 0Z"/>
  </svg>
)

const RefreshIcon = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
    <path d="M8 3a5 5 0 0 0-4.546 2.914.5.5 0 1 1-.908-.418A6 6 0 0 1 14 8a.5.5 0 0 1-1 0 5 5 0 0 0-5-5Z"/>
    <path d="M8 13a5 5 0 0 0 4.546-2.914.5.5 0 1 1 .908.418A6 6 0 0 1 2 8a.5.5 0 0 1 1 0 5 5 0 0 0 5 5Z"/>
  </svg>
)

export default forwardRef<FileBrowserHandle, FileBrowserProps>(function FileBrowser({ sessionId, connHost, jumpToPath, onTerminalCommand, onCdHere, onStartUpload }, ref) {
  const { t } = useTranslation()
  const [currentPath, setCurrentPath] = useState('/')
  const [files, setFiles] = useState<FileEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [cacheTime, setCacheTime] = useState<number>(0) // ponytail: cached_at ms
  const initializedRef = useRef(false)
  const [uploadMenuOpen, setUploadMenuOpen] = useState(false)
  const [selectedFiles, setSelectedFiles] = useState<Set<string>>(new Set())
  const [lastClickedFile, setLastClickedFile] = useState<string | null>(null)
  const [rubberBand, setRubberBand] = useState<{ startX: number; startY: number; endX: number; endY: number } | null>(null)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [contextMenu, setContextMenu] = useState<FileContextMenu | null>(null)
  const [clipboard, setClipboard] = useState<Clipboard | null>(null)
  const [operationLog, setOperationLog] = useState<{ lines: string[] } | null>(null)
  const [fileInfo, setFileInfo] = useState<FileInfo | null>(null)
  const [confirmDialog, setConfirmDialog] = useState<ConfirmDialog | null>(null)
  const [conflictDialog, setConflictDialog] = useState<ConflictDialog | null>(null)
  const [promptDialog, setPromptDialog] = useState<{ title: string; value: string; onSubmit: (v: string) => void } | null>(null)
  const [permissionDialog, setPermissionDialog] = useState<{ paths: string[]; names: string[]; currentPerms: string; mode: string } | null>(null)
  const [deleteLog, setDeleteLog] = useState<string | null>(null)
  const [archiveProgress, setArchiveProgress] = useState<{ type: string; logs: string[]; done: boolean } | null>(null)
  const [copyProgress, setCopyProgress] = useState<{ logs: string[]; done: boolean } | null>(null)
  const [bgContextMenu, setBgContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [downloadDialog, setDownloadDialog] = useState<{ url: string } | null>(null)
  const [downloadProgress, setDownloadProgress] = useState<{ progress: number; status: string } | null>(null)
  const [compressDialog, setCompressDialog] = useState<{ names: string[] } | null>(null)
  const [compressFormat, setCompressFormat] = useState<'zip' | 'tar.gz' | 'tar.bz2'>('zip')
  const [dropActive, setDropActive] = useState(false)
  const dragItemRef = useRef<FileEntry | null>(null)
  const [dragOverTarget, setDragOverTarget] = useState<string | null>(null)
  const [dragGhost, setDragGhost] = useState<{ name: string; isDir: boolean; x: number; y: number } | null>(null)
  const [draggingName, setDraggingName] = useState<string | null>(null)
  const [pathInputValue, setPathInputValue] = useState('/')
  const [searchQuery, setSearchQuery] = useState('')
  const dragStartPos = useRef<{ x: number; y: number } | null>(null)
  const isDragging = useRef(false)
  const pathInputRef = useRef<HTMLInputElement>(null)
  const gridRef = useRef<HTMLDivElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const bgMenuRef = useRef<HTMLDivElement>(null)
  const promptInputRef = useRef<HTMLInputElement>(null)
  const archiveLogRef = useRef<HTMLPreElement>(null)
  const copyLogRef = useRef<HTMLPreElement>(null)
  const operationLogRef = useRef<HTMLPreElement>(null)
  const favoritesDropdownRef = useRef<HTMLDivElement>(null)
  const fileBrowserRef = useRef<HTMLDivElement>(null)

  // Favorites management
  const [favorites, setFavorites] = useState<string[]>([])
  const [showFavoritesDropdown, setShowFavoritesDropdown] = useState(false)

  // ponytail: client-side filename filter — no backend round-trip
  const filteredFiles = useMemo(() => {
    if (!searchQuery) return files
    const q = searchQuery.toLowerCase()
    return files.filter(f => f.name.toLowerCase().includes(q))
  }, [files, searchQuery])

  // Helper: resolve full remote path for a file entry
  const resolvePath = useCallback((entry: FileEntry) =>
    currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
  , [currentPath])

  // Helper: get all currently selected FileEntry objects
  const getSelectedEntries = useCallback((): FileEntry[] =>
    files.filter(f => selectedFiles.has(f.name))
  , [files, selectedFiles])

  // Toast system → append to operation log floating panel
  const showToast = useCallback((message: string, type: Toast['type'] = 'info') => {
    const icon = type === 'success' ? '✅' : type === 'error' ? '❌' : 'ℹ️'
    setOperationLog(prev => ({ lines: [...(prev?.lines ?? []), `${icon} ${message}`] }))
  }, [])

  // Confirm dialog (replaces native confirm)
  const showConfirm = useCallback((message: string, onConfirm: () => void) => {
    setConfirmDialog({ message, onConfirm })
  }, [])

  // Prompt dialog (replaces native prompt)
  const showPrompt = useCallback((title: string, onSubmit: (v: string) => void, defaultValue = '') => {
    setPromptDialog({ title, value: defaultValue, onSubmit })
    setTimeout(() => {
      const input = promptInputRef.current
      if (input) {
        input.focus()
        input.select()
      }
    }, 50)
  }, [])

  // Conflict dialog (ask user to replace, rename, or skip)
  const showConflict = useCallback((item: ConflictItem, remaining: number): Promise<{ action: 'replace' | 'rename' | 'skip', applyToAll: boolean }> => {
    return new Promise((resolve) => {
      setConflictDialog({ item, remaining, resolve })
    })
  }, [])

  // Load favorites from SQLite
  useEffect(() => {
    if (sessionId) {
      invoke<string[]>('fb_favorites_list', { sessionId }).then(setFavorites).catch(() => {})
    }
  }, [sessionId])

  const addFavorite = useCallback((path: string) => {
    if (!favorites.includes(path)) {
      setFavorites(prev => [...prev, path])
      if (sessionId) invoke('fb_favorites_add', { sessionId, path }).catch(() => {})
      showToast(t('files.addedToFavorites', { path }), 'success')
    } else {
      showToast(t('files.alreadyInFavorites'), 'info')
    }
  }, [favorites, sessionId, showToast])

  const removeFavorite = useCallback((path: string) => {
    setFavorites(prev => prev.filter(p => p !== path))
    if (sessionId) invoke('fb_favorites_remove', { sessionId, path }).catch(() => {})
    showToast(t('files.removedFromFavorites', { path }), 'info')
  }, [sessionId, showToast])

  const isFavorite = useCallback((path: string) => {
    return favorites.includes(path)
  }, [favorites])

  const navigateTo = useCallback(async (path: string, forceRefresh?: boolean) => {
    if (!sessionId) return
    setSearchQuery('')
    setSelectedFiles(new Set())
    setLastClickedFile(null)
    const resolvedPath = path.startsWith('/')
      ? path
      : currentPath === '/' ? `/${path}` : `${currentPath}/${path}`

    // ponytail: SWR - show cache immediately, revalidate in background
    let cachedJson: string | null = null
    if (!forceRefresh) {
      try {
        const result = await invoke<[string, number] | null>('fb_cache_get', { sessionId, path: resolvedPath })
        if (result) {
          cachedJson = result[0]
          setCacheTime(result[1])
        }
      } catch { /* ignore */ }
    }

    if (cachedJson) {
      try {
        const cached: FileEntry[] = JSON.parse(cachedJson)
        cached.sort((a, b) => a.isDir !== b.isDir ? (a.isDir ? -1 : 1) : a.name.localeCompare(b.name))
        setFiles(cached)
        setCurrentPath(resolvedPath)
        setPathInputValue(resolvedPath)
      } catch { /* bad cache, fall through to loading */ }
    }

    setLoading(!cachedJson) // only show spinner if no cache
    try {
      const json = await invoke<string>('ssh_list_dir', { sessionId, path: resolvedPath })
      let entries: FileEntry[] = JSON.parse(json)
      entries.sort((a, b) => {
        if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
        return a.name.localeCompare(b.name)
      })
      // ponytail: update UI + cache if data changed or force refresh; otherwise just touch timestamp
      if (json !== cachedJson || forceRefresh) {
        setFiles(entries)
        setCurrentPath(resolvedPath)
        setPathInputValue(resolvedPath)
        invoke('fb_cache_put', { sessionId, path: resolvedPath, data: json, fileCount: entries.length }).catch(() => {})
        setCacheTime(Date.now())
      } else {
        // ponytail: data unchanged - refresh cache timestamp only
        invoke('fb_cache_touch', { sessionId, path: resolvedPath }).catch(() => {})
        setCacheTime(Date.now())
      }
      if (connHost) invoke('ui_state_set', { key: `fb_path_${connHost}`, value: resolvedPath }).catch(() => {})
      onTerminalCommand?.(`cd ${resolvedPath} && ls -la`)
    } catch (e) {
      console.error('list_dir error:', e)
    } finally {
      setLoading(false)
    }
  }, [sessionId, currentPath, connHost, onTerminalCommand])


  useImperativeHandle(ref, () => ({
    jumpToPath: (path: string) => navigateTo(path),
    refreshCurrentDirectory: () => navigateTo(currentPath)
  }), [navigateTo, currentPath])

  // Get unique filename in current directory
  const getUniqueName = useCallback((name: string) => {
    const existing = new Set(files.map(f => f.name))
    if (!existing.has(name)) return name
    const dotIdx = name.lastIndexOf('.')
    const base = dotIdx > 0 ? name.substring(0, dotIdx) : name
    const ext = dotIdx > 0 ? name.substring(dotIdx) : ''
    for (let i = 1; i < 1000; i++) {
      const candidate = `${base} (copy ${i})${ext}`
      if (!existing.has(candidate)) return candidate
    }
    return `${base}_copy${ext}`
  }, [files])

  // Check disk space and write permission before paste/download
  const checkDirReady = useCallback(async (): Promise<{ ok: boolean; existingFiles: Map<string, 'file' | 'dir'> }> => {
    if (!sessionId) return { ok: false, existingFiles: new Map() }
    try {
      const raw = await invoke<string>('ssh_check_space', { sessionId, path: currentPath })
      const parts = raw.split('---').map(s => s.trim())
      const availBytes = parseInt(parts[0]) || 0
      const writeOk = parts[1] === 'OK'
      // Parse file list with type code from find -printf '%y': d=dir, f=file, l=link, etc.
      const fileMap = new Map<string, 'file' | 'dir'>()
      ;(parts[2] || '').split('\n').filter(Boolean).forEach(line => {
        const lastPipe = line.lastIndexOf('|')
        if (lastPipe > 0) {
          const name = line.substring(0, lastPipe)
          const typeCode = line.substring(lastPipe + 1)
          fileMap.set(name, typeCode === 'd' ? 'dir' : 'file')
        }
      })

      if (!writeOk) {
        showToast(t('files.noWritePerm'), 'error')
        return { ok: false, existingFiles: fileMap }
      }
      if (availBytes < 1024) {
        showToast(t('files.insufficientSpace'), 'error')
        return { ok: false, existingFiles: fileMap }
      }
      return { ok: true, existingFiles: fileMap }
    } catch {
      // Fallback to current files array (without type info)
      const fallbackMap = new Map<string, 'file' | 'dir'>()
      files.forEach(f => fallbackMap.set(f.name, f.isDir ? 'dir' : 'file'))
      return { ok: true, existingFiles: fallbackMap }
    }
  }, [sessionId, currentPath, files, showToast])

  // Listen for download progress events
  useEffect(() => {
    const unlisten = listen<{ progress: number; status: string; error?: string }>('download-progress', (e) => {
      setDownloadProgress({ progress: e.payload.progress, status: e.payload.status })
      if (e.payload.status === 'done') {
        showToast(t('files.downloadComplete'), 'success')
        setTimeout(() => {
          setDownloadProgress(null)
          setDownloadDialog(null)
          navigateTo(currentPath)
        }, 800)
      } else if (e.payload.status === 'error') {
        showToast(t('files.downloadFailed', { error: e.payload.error || 'unknown' }), 'error')
        setTimeout(() => setDownloadProgress(null), 2000)
      }
    })
    return () => { unlisten.then(f => f()) }
  }, [currentPath, navigateTo, showToast]) // eslint-disable-line

  // Listen for archive progress events
  useEffect(() => {
    const unlisten = listen<{ sessionId: string; line: string; status: string }>('archive-progress', (e) => {
      setArchiveProgress(prev => {
        if (!prev) return prev
        const isDone = e.payload.status === 'done'
        return {
          ...prev,
          logs: [...prev.logs, e.payload.line],
          done: isDone || prev.done,
        }
      })
    })
    return () => { unlisten.then(f => f()) }
  }, [])

  // Listen for copy progress events
  useEffect(() => {
    const unlisten = listen<{ sessionId: string; line: string; status: string }>('copy-progress', (e) => {
      setCopyProgress(prev => {
        if (!prev) return prev
        return {
          ...prev,
          logs: [...prev.logs, e.payload.line],
          done: e.payload.status === 'error' || prev.done,
        }
      })
    })
    return () => { unlisten.then(f => f()) }
  }, [])

  // Auto-scroll archive log to bottom
  useEffect(() => {
    if (archiveLogRef.current) {
      archiveLogRef.current.scrollTop = archiveLogRef.current.scrollHeight
    }
  }, [archiveProgress?.logs.length])

  // Auto-scroll copy log to bottom
  useEffect(() => {
    if (copyLogRef.current) {
      copyLogRef.current.scrollTop = copyLogRef.current.scrollHeight
    }
  }, [copyProgress?.logs.length])

  // Auto-scroll operation log to bottom
  useEffect(() => {
    if (operationLogRef.current) {
      operationLogRef.current.scrollTop = operationLogRef.current.scrollHeight
    }
  }, [operationLog?.lines.length])

  // Auto-dismiss operation log 5s after last entry
  useEffect(() => {
    if (operationLog) {
      const timer = setTimeout(() => setOperationLog(null), 5000)
      return () => clearTimeout(timer)
    }
  }, [operationLog?.lines.length])

  // Auto-dismiss archive panel 5s after completion
  useEffect(() => {
    if (archiveProgress?.done) {
      const timer = setTimeout(() => setArchiveProgress(null), 5000)
      return () => clearTimeout(timer)
    }
  }, [archiveProgress?.done])

  // Auto-dismiss copy panel 5s after completion
  useEffect(() => {
    if (copyProgress?.done) {
      const timer = setTimeout(() => setCopyProgress(null), 5000)
      return () => clearTimeout(timer)
    }
  }, [copyProgress?.done])

  // Prevent WebView2 from intercepting file drops at native level
  // This MUST use native DOM listeners (not React synthetic events)
  // Only preventDefault — do NOT stopPropagation so React events still fire
  useEffect(() => {
    const prevent = (e: DragEvent) => {
      e.preventDefault()
    }
    window.addEventListener('dragover', prevent, false)
    window.addEventListener('drop', prevent, false)
    return () => {
      window.removeEventListener('dragover', prevent, false)
      window.removeEventListener('drop', prevent, false)
    }
  }, [])

  // Handle files dropped from local computer or selected via file picker
  const handleUploadFiles = useCallback(async (items: FileList) => {
    if (!sessionId) return
    if (!items || items.length === 0) return

    const { ok, existingFiles } = await checkDirReady()
    if (!ok) return

    // Collect conflict names (files that already exist on remote)
    const conflictNames: string[] = []
    for (let i = 0; i < items.length; i++) {
      if (existingFiles.has(items[i].name)) {
        conflictNames.push(items[i].name)
      }
    }

    // Resolve conflicts: map name -> 'replace' | 'rename' | 'skip'
    const resolutions = new Map<string, 'replace' | 'rename' | 'skip'>()
    if (conflictNames.length > 0) {
      let globalAction: 'replace' | 'rename' | 'skip' | null = null
      for (let i = 0; i < conflictNames.length; i++) {
        const name = conflictNames[i]
        if (globalAction) {
          // Apply the global action chosen by user
          resolutions.set(name, globalAction)
        } else {
          const fileType = existingFiles.get(name)
          const isDir = fileType === 'dir'
          const remaining = conflictNames.length - i - 1
          const result = await showConflict({ name, isDir }, remaining)
          resolutions.set(name, result.action)
          if (result.applyToAll) {
            globalAction = result.action
          }
        }
      }
    }

    // Upload each file respecting resolutions
    const uploadFiles: { file: File; fileName: string; remotePath: string }[] = []
    for (let i = 0; i < items.length; i++) {
      const file = items[i]
      let fileName = file.name
      const resolution = resolutions.get(fileName)
      if (resolution === 'skip') continue
      if (resolution === 'rename') {
        fileName = getUniqueName(fileName)
      }
      const remotePath = currentPath === '/' ? `/${fileName}` : `${currentPath}/${fileName}`
      uploadFiles.push({ file, fileName, remotePath })
    }

    if (uploadFiles.length === 0) return
    onStartUpload?.(uploadFiles)
    navigateTo(currentPath)
  }, [sessionId, currentPath, checkDirReady, getUniqueName, navigateTo, showToast, showConflict, onStartUpload])

 // ponytail: Tauri native dialog for folder upload — avoids webkitdirectory WebView2 issues
  const walkDir = async (dir: string, relBase: string): Promise<{ files: { absPath: string; relPath: string }[]; dirs: string[] }> => {
    const entries = await readDir(dir)
    const files: { absPath: string; relPath: string }[] = []
    const dirs: string[] = []
    for (const entry of entries) {
      if (!entry.name) continue
      const absPath = dir + '/' + entry.name
      const relPath = relBase ? relBase + '/' + entry.name : entry.name
      if (entry.isDirectory) {
        dirs.push(relPath)
        const sub = await walkDir(absPath, relPath)
        files.push(...sub.files)
        dirs.push(...sub.dirs)
      } else if (entry.isFile) {
        files.push({ absPath, relPath })
      }
    }
    return { files, dirs }
  }

  // ponytail: shared folder upload logic — create dirs + upload files with relative paths
  const uploadFolderFiles = useCallback(async (files: { file: File; relPath: string }[], extraDirs: string[] = []) => {
    // Create directory structure on server
    const dirsToCreate = new Set<string>(extraDirs.map(d => currentPath === '/' ? '/' + d : currentPath + '/' + d))
    for (const f of files) {
      const parts = f.relPath.split('/')
      for (let j = 1; j < parts.length; j++) {
        const dirPath = currentPath === '/'
          ? '/' + parts.slice(0, j).join('/')
          : currentPath + '/' + parts.slice(0, j).join('/')
        dirsToCreate.add(dirPath)
      }
    }
    const sortedDirs = [...dirsToCreate].sort((a, b) => a.split('/').length - b.split('/').length)
    for (const dir of sortedDirs) {
      try { await invoke('ssh_create_dir', { sessionId, path: dir }) } catch (_) { /* may exist */ }
    }
    const uploadList: { file: File; fileName: string; remotePath: string }[] = []
    for (const f of files) {
      const remotePath = currentPath === '/' ? `/${f.relPath}` : `${currentPath}/${f.relPath}`
      uploadList.push({ file: f.file, fileName: f.relPath, remotePath })
    }
    if (uploadList.length > 0) {
      onStartUpload?.(uploadList)
    }
    navigateTo(currentPath)
  }, [sessionId, currentPath, navigateTo, onStartUpload])

  // ponytail: recursively traverse FileSystemDirectoryEntry from drag-drop
  const readEntries = (reader: FileSystemDirectoryReader): Promise<FileSystemEntry[]> =>
    new Promise((resolve, reject) => reader.readEntries(resolve, reject))

  const walkDragEntries = async (entry: FileSystemEntry, prefix = ''): Promise<{ files: { file: File; relPath: string }[]; dirs: string[] }> => {
    if (entry.isFile) {
      return new Promise((resolve) => {
        (entry as FileSystemFileEntry).file(f => resolve({ files: [{ file: f, relPath: prefix + entry.name }], dirs: [] }), () => resolve({ files: [], dirs: [] }))
      })
    }
    if (entry.isDirectory) {
      const dirPath = prefix + entry.name
      const reader = (entry as FileSystemDirectoryEntry).createReader()
      const allEntries: FileSystemEntry[] = []
      let batch: FileSystemEntry[]
      do {
        batch = await readEntries(reader)
        allEntries.push(...batch)
      } while (batch.length > 0)
      const results = await Promise.all(allEntries.map(e => walkDragEntries(e, dirPath + '/')))
      // ponytail: include this dir itself so empty dirs get created
      return { files: results.flatMap(r => r.files), dirs: [dirPath, ...results.flatMap(r => r.dirs)] }
    }
    return { files: [], dirs: [] }
  }

  const handleUploadFolder = useCallback(async () => {
    if (!sessionId) return
    const folderPath = await openDialog({ directory: true, multiple: false })
    if (!folderPath || typeof folderPath !== 'string') return

    // Extract folder name from path
    const folderName = folderPath.split(/[\\/]/).pop()!
    const { files: allFiles, dirs: allDirs } = await walkDir(folderPath, folderName)

    // Read files from disk and create File objects
    const files: { file: File; relPath: string }[] = []
    for (const f of allFiles) {
      try {
        const bytes = await readFile(f.absPath)
        const blob = new Blob([bytes])
        const file = new File([blob], f.relPath, { type: 'application/octet-stream' })
        files.push({ file, relPath: f.relPath })
      } catch (_) { /* skip unreadable files */ }
    }
    if (files.length > 0 || allDirs.length > 0) await uploadFolderFiles(files, allDirs)
  }, [sessionId, currentPath, navigateTo, onStartUpload, uploadFolderFiles])

  // ponytail: Tauri native dialog for file upload — more reliable than hidden input.click() in WebView2
  const handleUploadFilesBtn = useCallback(async () => {
    if (!sessionId) return
    const selected = await openDialog({ multiple: true, directory: false })
    if (!selected) return
    const paths = Array.isArray(selected) ? selected : [selected]
    if (paths.length === 0) return

    const uploadFiles: { file: File; fileName: string; remotePath: string }[] = []
    for (const p of paths) {
      try {
        const fileName = p.split(/[\\/]/).pop()!
        const bytes = await readFile(p)
        const blob = new Blob([bytes])
        const file = new File([blob], fileName, { type: 'application/octet-stream' })
        const remotePath = currentPath === '/' ? `/${fileName}` : `${currentPath}/${fileName}`
        uploadFiles.push({ file, fileName, remotePath })
      } catch (_) { /* skip unreadable files */ }
    }

    if (uploadFiles.length === 0) return
    onStartUpload?.(uploadFiles)
    navigateTo(currentPath)
  }, [sessionId, currentPath, navigateTo, onStartUpload])

  // Close upload menu on outside click
  useEffect(() => {
    if (!uploadMenuOpen) return
    const handleClick = () => setUploadMenuOpen(false)
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [uploadMenuOpen])

  // Handle files/directories dropped from local computer
  const handleDropFiles = useCallback(async (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDropActive(false)
    // Check for directory entries via Web API
    const dirEntries: FileSystemEntry[] = []
    const fileItems: DataTransferItem[] = []
    for (let i = 0; i < e.dataTransfer.items.length; i++) {
      const item = e.dataTransfer.items[i]
      const entry = item.webkitGetAsEntry?.()
      if (entry?.isDirectory) {
        dirEntries.push(entry)
      } else {
        fileItems.push(item)
      }
    }
    // Upload regular files
    if (fileItems.length > 0) {
      const files = new DataTransfer()
      for (const item of fileItems) {
        const f = item.getAsFile()
        if (f) files.items.add(f)
      }
      if (files.files.length > 0) await handleUploadFiles(files.files)
    }
    // Upload directories using Web API traversal + shared upload logic
    if (dirEntries.length > 0) {
      for (const entry of dirEntries) {
        // ponytail: walkDragEntries tracks relPath manually and collects empty dirs
        const { files, dirs } = await walkDragEntries(entry)
        if (files.length === 0 && dirs.length === 0) continue
        await uploadFolderFiles(files, dirs)
      }
    }
  }, [handleUploadFiles, uploadFolderFiles])

  // Track drag-enter/leave with a counter to avoid flicker on child elements
  const dragCounterRef = useRef(0)

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
  }, [])

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    if (e.dataTransfer.types.includes('Files')) {
      dragCounterRef.current += 1
      setDropActive(true)
    }
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    dragCounterRef.current -= 1
    if (dragCounterRef.current <= 0) {
      dragCounterRef.current = 0
      setDropActive(false)
    }
  }, [])

  useEffect(() => {
    if (!sessionId) return
    // Use jumpToPath prop to navigate (triggered when value changes)
    if (jumpToPath) {
      setLoading(true)
      navigateTo(jumpToPath)
      initializedRef.current = true
      return
    }
    // If not initialized yet, restore saved path on first mount
    if (!initializedRef.current && !loading) {
      setLoading(true)
      initializedRef.current = true
      ;(async () => {
        const saved = connHost ? await invoke<string>('ui_state_get', { key: `fb_path_${connHost}` }).catch(() => '') : ''
        const target = saved || await invoke<string>('ssh_get_cwd', { sessionId }).catch(() => '/root')
        navigateTo(target, true)
      })()
    }
  }, [sessionId, jumpToPath]) // eslint-disable-line

  // Close suggestions when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (favoritesDropdownRef.current && !favoritesDropdownRef.current.contains(e.target as Node)) {
        setShowFavoritesDropdown(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  // Path edit: validate and go
  const handleGoToPath = async () => {
    if (!sessionId) return
    const p = pathInputValue.trim() || '/'
    const target = p.startsWith('/') ? p : '/' + p
    try {
      await invoke<string>('ssh_list_dir', { sessionId, path: target })
      // Path exists, navigate
      navigateTo(target)
    } catch {
      showToast(t('files.dirNotFound', { path: target }), 'error')
      // Keep input open, content unchanged
    }
  }

  // Pointer-based drag and drop (reliable in WebView2)
  const handleItemMouseDown = (e: React.MouseEvent, entry: FileEntry) => {
    if (e.button !== 0) return // left button only
    dragItemRef.current = entry
    dragStartPos.current = { x: e.clientX, y: e.clientY }
    isDragging.current = false
  }

  // Global mousemove/mouseup for drag
  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragItemRef.current || !dragStartPos.current) return
      const dx = e.clientX - dragStartPos.current.x
      const dy = e.clientY - dragStartPos.current.y
      // Require 5px movement before starting drag (avoid accidental drags on click)
      if (!isDragging.current) {
        if (Math.abs(dx) < 5 && Math.abs(dy) < 5) return
        isDragging.current = true
        document.body.classList.add('dragging-files')
        setDraggingName(dragItemRef.current.name)
      }
      setDragGhost({ name: dragItemRef.current.name, isDir: dragItemRef.current.isDir, x: e.clientX, y: e.clientY })

      // Find drop target using elementFromPoint
      const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null
      if (el) {
        const fbItem = el.closest('[data-fb-name]') as HTMLElement | null
        if (fbItem) {
          const targetName = fbItem.dataset.fbName || ''
          if (targetName && targetName !== dragItemRef.current.name) {
            setDragOverTarget(targetName)
            return
          }
        }
      }
      setDragOverTarget(null)
    }

    const onMouseUp = async (e: MouseEvent) => {
      if (!dragItemRef.current) return
      const dragged = dragItemRef.current
      const wasDragging = isDragging.current

      // Reset state
      dragItemRef.current = null
      dragStartPos.current = null
      isDragging.current = false
      setDragGhost(null)
      setDraggingName(null)
      document.body.classList.remove('dragging-files')

      if (!wasDragging) {
        setDragOverTarget(null)
        return
      }

      // Find drop target
      const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null
      setDragOverTarget(null)
      if (!el) return

      const fbItem = el.closest('[data-fb-name]') as HTMLElement | null
      if (!fbItem) return

      const targetName = fbItem.dataset.fbName || ''
      if (!targetName || targetName === dragged.name) return

      const targetIsDir = fbItem.dataset.fbIsdir === 'true'

      if (targetIsDir) {
        // Move file(s) into folder — move all selected items if dragged item is selected
        if (!sessionId) return
        const toMove = selectedFiles.has(dragged.name) && selectedFiles.size > 1
          ? files.filter(f => selectedFiles.has(f.name))
          : [dragged]
        let ok = 0, fail = 0
        for (const item of toMove) {
          const srcPath = currentPath === '/' ? `/${item.name}` : `${currentPath}/${item.name}`
          const dstPath = currentPath === '/' ? `/${targetName}/${item.name}` : `${currentPath}/${targetName}/${item.name}`
          try {
            await invoke('ssh_rename_file', { sessionId, oldPath: srcPath, newPath: dstPath })
            ok++
          } catch { fail++ }
        }
        if (fail === 0) showToast(t('files.movedItems', { count: ok }), 'success')
        else showToast(t('files.moveFailed', { ok, fail }), 'error')
        setSelectedFiles(new Set())
        navigateTo(currentPath)
      } else {
        // Reorder files in the local array
        setFiles(prev => {
          const newFiles = [...prev]
          const dragIdx = newFiles.findIndex(f => f.name === dragged.name)
          const targetIdx = newFiles.findIndex(f => f.name === targetName)
          if (dragIdx < 0 || targetIdx < 0) return prev
          const [removed] = newFiles.splice(dragIdx, 1)
          newFiles.splice(targetIdx, 0, removed)
          return newFiles
        })
      }
    }

    window.addEventListener('mousemove', onMouseMove)
    window.addEventListener('mouseup', onMouseUp)
    return () => {
      window.removeEventListener('mousemove', onMouseMove)
      window.removeEventListener('mouseup', onMouseUp)
    }
  }, [sessionId, currentPath, showToast, navigateTo, selectedFiles, files])

  const handleItemClick = (entry: FileEntry, e: React.MouseEvent) => {
    if (e.ctrlKey || e.metaKey) {
      // Toggle individual selection
      setSelectedFiles(prev => {
        const next = new Set(prev)
        if (next.has(entry.name)) next.delete(entry.name)
        else next.add(entry.name)
        return next
      })
      setLastClickedFile(entry.name)
    } else if (e.shiftKey && lastClickedFile) {
      // Range selection
      const lastIdx = files.findIndex(f => f.name === lastClickedFile)
      const currIdx = files.findIndex(f => f.name === entry.name)
      if (lastIdx >= 0 && currIdx >= 0) {
        const [start, end] = [Math.min(lastIdx, currIdx), Math.max(lastIdx, currIdx)]
        setSelectedFiles(new Set(files.slice(start, end + 1).map(f => f.name)))
      }
    } else {
      // Single select
      setSelectedFiles(new Set([entry.name]))
      setLastClickedFile(entry.name)
    }
  }

  const handleItemDoubleClick = (entry: FileEntry) => {
    if (entry.isDir) {
      const newPath = currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
      navigateTo(newPath)
    } else if (isImageFile(entry.name)) {
      openImageLocal(entry)
    } else {
      openEditor(entry)
    }
  }

  const openImageLocal = async (entry: FileEntry) => {
    if (!sessionId) return
    const filePath = currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
    showToast(t('files.downloadingFile', { name: entry.name }), 'info')
    try {
      const localPath = await invoke<string>('ssh_download_to_local', {
        sessionId,
        remotePath: filePath,
        fileName: entry.name,
      })
      showToast(t('files.openImage', { name: entry.name }), 'success')
      onTerminalCommand?.(`# Downloaded ${filePath} -> ${localPath}`)
    } catch (e) {
      showToast(t('files.failedOpenImage', { error: e }), 'error')
    }
  }

  const openEditor = async (entry: FileEntry) => {
    // ponytail: allow all files up to 3MB, no format restriction
    if (entry.size >= 3 * 1024 * 1024) {
      showToast(t('files.binaryOrLarge'), 'info')
      onTerminalCommand?.(`file ${currentPath}/${entry.name}`)
      return
    }
    const filePath = currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
    try {
      const content = await invoke<string>('ssh_read_file', { sessionId, path: filePath })
      setEditor({ path: filePath, name: entry.name, content, originalContent: content, saving: false })
      onTerminalCommand?.(`cat ${filePath}`)
    } catch (e) {
      console.error('read_file error:', e)
      showToast(t('files.readFailed', { error: e }), 'error')
    }
  }

  const handleSaveFile = async () => {
    if (!editor || !sessionId) return
    setEditor({ ...editor, saving: true })
    try {
      await invoke('ssh_write_file', { sessionId, path: editor.path, content: editor.content })
      setEditor({ ...editor, originalContent: editor.content, saving: false })
      showToast(t('files.savedFile', { name: editor.name }), 'success')
      onTerminalCommand?.(`# Saved ${editor.path}`)
    } catch (e) {
      showToast(t('files.saveFailedMsg', { error: e }), 'error')
      setEditor({ ...editor, saving: false })
    }
  }

  const handleDelete = () => {
    const entries = getSelectedEntries()
    if (entries.length === 0) return
    const msg = entries.length === 1
      ? t('files.deleteFileMsg', { type: entries[0].isDir ? t('files.folder') : t('files.file'), name: entries[0].name })
      : t('files.deleteItemsMsg', { count: entries.length })
    showConfirm(msg, async () => {
      try {
        // Separate files and directories
        const files = entries.filter(e => !e.isDir).map(e => resolvePath(e))
        const dirs = entries.filter(e => e.isDir).map(e => resolvePath(e))

        const logs: string[] = []

        // Delete files first (single command)
        if (files.length > 0) {
          const output = await invoke<string>('ssh_delete_files_batch', {
            sessionId,
            paths: files,
            isDir: false,
          })
          if (output) logs.push(output.trim())
        }

        // Then delete directories (single command)
        if (dirs.length > 0) {
          const output = await invoke<string>('ssh_delete_files_batch', {
            sessionId,
            paths: dirs,
            isDir: true,
          })
          if (output) logs.push(output.trim())
        }

        if (logs.length > 0) {
          setDeleteLog(logs.join('\n'))
          setTimeout(() => setDeleteLog(null), 5000)
        }

        showToast(t('files.deletedItems', { count: entries.length }), 'success')
        setSelectedFiles(new Set())
        navigateTo(currentPath)
      } catch (e) {
        showToast(`Delete failed: ${e}`, 'error')
      }
    })
  }

  const handleNewFile = () => {
    showPrompt(t('files.newFileName'), async (name) => {
      if (!name) return
      const filePath = currentPath === '/' ? `/${name}` : `${currentPath}/${name}`
      try {
        await invoke('ssh_write_file', { sessionId, path: filePath, content: '' })
        showToast(t('files.createdFile', { name }), 'success')
        navigateTo(currentPath)
      } catch (e) {
        showToast(t('files.createFailed', { error: e }), 'error')
      }
    })
  }

  const handleNewFolder = () => {
    showPrompt(t('files.newFolderName'), async (name) => {
      if (!name) return
      const dirPath = currentPath === '/' ? `/${name}` : `${currentPath}/${name}`
      try {
        await invoke('ssh_create_dir', { sessionId, path: dirPath })
        showToast(t('files.createdFolder', { name }), 'success')
        navigateTo(currentPath)
      } catch (e) {
        showToast(t('files.createFolderFailed', { error: e }), 'error')
      }
    })
  }

  const handleCopy = (entries?: FileEntry[]) => {
    const items = entries || getSelectedEntries()
    if (items.length === 0) return
    const paths = items.map(e => resolvePath(e))
    const names = items.map(e => e.name)
    const isDirs = items.map(e => e.isDir)
    const hasDirs = items.some(e => e.isDir)
    setClipboard({ paths, names, isDirs, hasDirs, mode: 'copy' })
    showToast(t('files.copiedItems', { count: items.length }), 'info')
  }

  const handleCut = (entries?: FileEntry[]) => {
    const items = entries || getSelectedEntries()
    if (items.length === 0) return
    const paths = items.map(e => resolvePath(e))
    const names = items.map(e => e.name)
    const isDirs = items.map(e => e.isDir)
    const hasDirs = items.some(e => e.isDir)
    setClipboard({ paths, names, isDirs, hasDirs, mode: 'cut' })
    showToast(t('files.cutItems', { count: items.length }), 'info')
  }

  const handlePaste = async () => {
    if (!clipboard || !sessionId) return
    const { ok, existingFiles } = await checkDirReady()
    if (!ok) return

    // Resolve paste conflicts
    const pasteConflicts: string[] = []
    for (let i = 0; i < clipboard.names.length; i++) {
      if (existingFiles.has(clipboard.names[i])) {
        pasteConflicts.push(clipboard.names[i])
      }
    }
    const pasteResolutions = new Map<string, 'replace' | 'rename' | 'skip'>()
    if (pasteConflicts.length > 0) {
      let globalAction: 'replace' | 'rename' | 'skip' | null = null
      for (let i = 0; i < pasteConflicts.length; i++) {
        const name = pasteConflicts[i]
        if (globalAction) {
          pasteResolutions.set(name, globalAction)
        } else {
          // Use file type from existingFiles map instead of searching in files array
          const fileType = existingFiles.get(name)
          const isDir = fileType === 'dir'
          const remaining = pasteConflicts.length - i - 1
          const result = await showConflict({ name, isDir }, remaining)
          pasteResolutions.set(name, result.action)
          if (result.applyToAll) {
            globalAction = result.action
          }
        }
      }
    }

    let success = 0, fail = 0
    const isCopy = clipboard.mode === 'copy'
    if (isCopy) {
      setCopyProgress({ logs: [], done: false })
    }

    // Check if we can use batch operation (no conflicts and all same type)
    const hasConflicts = pasteConflicts.length > 0
    const allFiles = clipboard.isDirs.every(d => !d) // all are files
    const allDirs = clipboard.isDirs.every(d => d)   // all are directories
    const canBatch = !hasConflicts && (allFiles || allDirs) && clipboard.paths.length > 1

    if (canBatch) {
      // Use batch API for better performance
      try {
        const output = await invoke<string>('ssh_copy_files_batch', {
          sessionId,
          sources: clipboard.paths,
          destDir: currentPath,
          isMove: clipboard.mode === 'cut'
        })
        
        // Parse output to count successes/failures
        const lines = output.trim().split('\n').filter(l => l.trim())
        success = lines.length
        fail = 0
        
        if (isCopy) {
          setCopyProgress(prev => prev ? { ...prev, done: true } : prev)
        }
        if (clipboard.mode === 'cut') setClipboard(null)
        showToast(t('files.pastedItems', { count: success }), 'success')
        navigateTo(currentPath)
        return
      } catch (e) {
        // Fallback to individual operations on error
        console.error('Batch copy failed, falling back to individual:', e)
        showToast(`Batch operation failed: ${e}`, 'error')
        // Continue to individual processing below
      }
    }

    // Execute all copy/move operations concurrently (fallback or when batch not applicable)
    const results = await Promise.allSettled(
      clipboard.paths.map(async (srcPath, i) => {
        let name = clipboard.names[i]
        const resolution = pasteResolutions.get(name)
        if (resolution === 'skip') {
          return { status: 'skipped' as const }
        }
        if (resolution === 'rename') {
          name = getUniqueName(name)
        }
        const dstPath = currentPath === '/' ? `/${name}` : `${currentPath}/${name}`
        if (srcPath === dstPath && clipboard.mode === 'cut') {
          return { status: 'skipped' as const }
        }

        try {
          if (clipboard.mode === 'copy') {
            if (clipboard.isDirs[i]) {
              await invoke('ssh_copy_dir', { sessionId, src: srcPath, dst: dstPath })
            } else {
              await invoke('ssh_copy_file', { sessionId, src: srcPath, dst: dstPath })
            }
          } else {
            await invoke('ssh_rename_file', { sessionId, oldPath: srcPath, newPath: dstPath })
          }
          return { status: 'success' as const }
        } catch (e) {
          return { status: 'failed' as const, error: e, name: clipboard.names[i] }
        }
      })
    )

    // Count results
    results.forEach(result => {
      if (result.status === 'fulfilled') {
        if (result.value.status === 'success') {
          success++
        } else if (result.value.status === 'failed') {
          fail++
          showToast(`Copy failed: ${result.value.name} — ${result.value.error}`, 'error')
        }
      } else {
        fail++
      }
    })

    if (isCopy) {
      setCopyProgress(prev => prev ? { ...prev, done: true } : prev)
    }
    if (clipboard.mode === 'cut') setClipboard(null)
    if (fail === 0) showToast(t('files.pastedItems', { count: success }), 'success')
    else showToast(t('files.pasteFailed', { ok: success, fail }), 'error')
    navigateTo(currentPath)
  }

  const handleRename = (entry: FileEntry) => {
    const oldPath = currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
    showPrompt(t('files.renameTitle', { name: entry.name }), async (newName) => {
      if (!newName || newName === entry.name) return
      const newPath = currentPath === '/' ? `/${newName}` : `${currentPath}/${newName}`
      try {
        await invoke('ssh_rename_file', { sessionId, oldPath, newPath })
        showToast(t('files.renamedTo', { name: newName }), 'success')
        navigateTo(currentPath)
      } catch (e) {
        showToast(t('files.renameFailed', { error: e }), 'error')
      }
    }, entry.name)
  }

  const handleShowInfo = (entry: FileEntry) => {
    const path = currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
    setFileInfo({ entry, path })
  }

  const handleDownload = async () => {
    if (!downloadDialog?.url || !sessionId) return
    const url = downloadDialog.url.trim()
    if (!url) return

    const { ok, existingFiles } = await checkDirReady()
    if (!ok) return

    // Extract filename from URL
    let fileName = 'download'
    try {
      const urlPath = new URL(url).pathname
      const last = urlPath.split('/').filter(Boolean).pop()
      if (last) fileName = decodeURIComponent(last)
    } catch { /* use default */ }

    if (existingFiles.has(fileName)) {
      const fileType = existingFiles.get(fileName)
      const isDir = fileType === 'dir'
      const result = await showConflict({ name: fileName, isDir }, 0)
      if (result.action === 'skip') {
        return
      }
      if (result.action === 'rename') {
        fileName = getUniqueName(fileName)
      }
      // 'replace' uses the original fileName
    }
    const destPath = currentPath === '/' ? `/${fileName}` : `${currentPath}/${fileName}`

    setDownloadProgress({ progress: 0, status: 'starting' })
    showToast(t('files.startingDownload', { name: fileName }), 'info')

    try {
      await invoke('ssh_download_file', { sessionId, url, dest: destPath })
      showToast(t('files.downloadedFile', { name: fileName }), 'success')
      setDownloadProgress(null)
      setDownloadDialog(null)
      // Refresh directory to show the downloaded file
      navigateTo(currentPath)
    } catch (e) {
      showToast(t('files.downloadError', { error: e }), 'error')
      setDownloadProgress(null)
    }
  }

  const handleOpenPermissions = (entry?: FileEntry) => {
    // If no entry provided, use selected files
    let paths: string[]
    let names: string[]
    let currentPerms: string

    if (entry) {
      // Single file from context menu
      const path = currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`
      paths = [path]
      names = [entry.name]
      currentPerms = entry.permissions || '—'
    } else if (selectedFiles.size > 0) {
      // Multiple selected files
      const entries = getSelectedEntries()
      paths = entries.map(e => currentPath === '/' ? `/${e.name}` : `${currentPath}/${e.name}`)
      names = entries.map(e => e.name)
      currentPerms = entries[0]?.permissions || '—' // Show first file's permissions as reference
    } else {
      return // No files to set permissions on
    }

    setPermissionDialog({ paths, names, currentPerms, mode: '' })
  }

  const handleApplyPermissions = async () => {
    if (!permissionDialog || !permissionDialog.mode) return
    try {
      // Use batch API if multiple files, otherwise single file API
      if (permissionDialog.paths.length > 1) {
        await invoke('ssh_set_permissions_batch', {
          sessionId,
          paths: permissionDialog.paths,
          mode: permissionDialog.mode
        })
        showToast(t('files.permChangedBatch', { count: permissionDialog.paths.length, mode: permissionDialog.mode }), 'success')
      } else {
        await invoke('ssh_set_permissions', { sessionId, path: permissionDialog.paths[0], mode: permissionDialog.mode })
        showToast(t('files.permChanged', { name: permissionDialog.names[0], mode: permissionDialog.mode }), 'success')
      }
      setPermissionDialog(null)
      navigateTo(currentPath)
    } catch (e) {
      showToast(t('files.permFailed', { error: e }), 'error')
    }
  }

  const handleSaveAs = async (entry: FileEntry) => {
    if (!sessionId || entry.isDir) return
    const filePath = resolvePath(entry)
    showToast(t('files.preparingDownload', { name: entry.name }), 'info')
    try {
      const localPath = await invoke<string>('ssh_save_as_local', {
        sessionId,
        remotePath: filePath,
        fileName: entry.name,
      })
      showToast(t('files.savedTo', { path: localPath }), 'success')
    } catch (e) {
      if (String(e) !== 'Save cancelled') {
        showToast(t('files.saveFailed', { error: e }), 'error')
      }
    }
  }

  const isArchive = (name: string) => /\.(tar\.gz|tgz|tar\.bz2|tbz2|tar\.xz|txz|tar|zip)$/i.test(name)

  const handleCompress = async (names: string[], archiveName: string, format: string) => {
    if (!sessionId || names.length === 0) return
    setCompressDialog(null)
    const ext = /\.(tar\.gz|tgz|tar\.bz2|tbz2|tar\.xz|txz|tar|zip)$/i.test(archiveName)
      ? ''
      : `.${format}`
    const outputName = archiveName + ext
    const outputPath = currentPath === '/' ? `/${outputName}` : `${currentPath}/${outputName}`
    const paths = names.map(n => currentPath === '/' ? `/${n}` : `${currentPath}/${n}`)
    setArchiveProgress({ type: 'compress', logs: [t('files.compressingItems', { count: names.length })], done: false })
    try {
      await invoke('ssh_compress', { sessionId, paths, output: outputPath, format })
      showToast(t('files.archiveCreated', { name: outputName }), 'success')
      navigateTo(currentPath)
    } catch (e) {
      setArchiveProgress(prev => prev ? { ...prev, logs: [...prev.logs, `Error: ${e}`], done: true } : null)
      showToast(t('files.compressFailed', { error: e }), 'error')
    }
  }

  const handleExtract = (entry: FileEntry) => {
    if (!sessionId) return
    showPrompt(t('files.extractTo'), async (destDir) => {
      if (!destDir) return
      // Derive suggested folder name from archive filename
      let extractName = entry.name
        .replace(/\.(tar\.gz|tgz|tar\.bz2|tbz2|tar\.xz|txz|tar|zip)$/i, '')
      if (!extractName) extractName = 'extracted'

      const targetPath = destDir === '/' ? `/${extractName}` : `${destDir}/${extractName}`

      // Check if target already exists and determine its type (file or dir)
      let conflict = false
      let isTargetDir = true
      try {
        const stat = await invoke<any>('ssh_stat_file', { sessionId, path: targetPath })
        conflict = stat.exists
        isTargetDir = stat.isDir
      } catch {
        // Path doesn't exist — no conflict
      }

      let finalName = extractName
      if (conflict) {
        const result = await showConflict({ name: extractName, isDir: isTargetDir }, 0)
        if (result.action === 'skip') {
          showToast(t('files.skipped', { name: entry.name }), 'info')
          return
        }
        if (result.action === 'rename') {
          const newName = await new Promise<string>((resolve) => {
            showPrompt(t('files.folderName'), resolve, extractName + '_1')
          })
          if (!newName) return
          finalName = newName
        }
        // 'replace' uses original extractName
      }

      const archivePath = resolvePath(entry)
      // Build final extraction path with user-specified name
      const extractDestPath = destDir === '/' ? `/${finalName}` : `${destDir}/${finalName}`
      setArchiveProgress({ type: 'extract', logs: [t('files.extractingFile', { name: entry.name })], done: false })

      try {
        // If conflict and user chose replace, delete existing first
        if (conflict && finalName === extractName) {
          await invoke('ssh_delete_file', { sessionId, path: extractDestPath, isDir: isTargetDir })
        }

        // Extract directly to destination directory with the chosen name
        await invoke('ssh_extract', { sessionId, archivePath, destDir: extractDestPath })

        showToast(t('files.extracted', { name: entry.name, dest: finalName }), 'success')
        navigateTo(currentPath)
      } catch (e) {
        setArchiveProgress(prev => prev ? { ...prev, logs: [...prev.logs, `Error: ${e}`], done: true } : null)
        showToast(t('files.extractFailed', { error: e }), 'error')
      }
    }, currentPath)
  }

  const handleItemContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault()
    // If right-clicked item is not in selection, select only it
    if (!selectedFiles.has(entry.name)) {
      setSelectedFiles(new Set([entry.name]))
      setLastClickedFile(entry.name)
    }
    setContextMenu({ x: e.clientX, y: e.clientY, entry })
  }

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setContextMenu(null)
      }
      if (bgMenuRef.current && !bgMenuRef.current.contains(e.target as Node)) {
        setBgContextMenu(null)
      }
    }
    if (contextMenu || bgContextMenu) {
      window.addEventListener('mousedown', handleClick)
      return () => window.removeEventListener('mousedown', handleClick)
    }
  }, [contextMenu, bgContextMenu])

  // Keyboard shortcuts: F5 refresh, Ctrl+A select all, Ctrl+C/X/V copy/cut/paste, Delete
  // Only intercept when focus is within FileBrowser component
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Check if the event target is within the FileBrowser component
      const target = e.target as Node
      if (!fileBrowserRef.current || !fileBrowserRef.current.contains(target)) {
        return // Don't intercept if focus is outside FileBrowser
      }
      // Don't intercept shortcuts when focus is in an editable element
      const el = e.target as HTMLElement
      if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable) return
      
      if (e.key === 'F5') {
        e.preventDefault()
        navigateTo(currentPath)
      }
      if (e.ctrlKey && e.key === 'a') {
        e.preventDefault()
        setSelectedFiles(new Set(files.map(f => f.name)))
      }
      if (e.ctrlKey && e.key === 'c' && selectedFiles.size > 0) {
        e.preventDefault()
        handleCopy()
      }
      if (e.ctrlKey && e.key === 'x' && selectedFiles.size > 0) {
        e.preventDefault()
        handleCut()
      }
      if (e.ctrlKey && e.key === 'v' && clipboard) {
        e.preventDefault()
        handlePaste()
      }
      if (e.key === 'Delete' && selectedFiles.size > 0) {
        handleDelete()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [currentPath, selectedFiles, files, navigateTo]) // eslint-disable-line

  const goUp = () => {
    if (currentPath === '/') return
    const parts = currentPath.split('/')
    parts.pop()
    navigateTo(parts.join('/') || '/')
  }

  // Rubber band selection on grid empty area
  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      setRubberBand(prev => prev ? { ...prev, endX: e.clientX, endY: e.clientY } : null)
      // Calculate intersection with file items
      if (!gridRef.current) return
      const rb = { startX: rubberBand?.startX || 0, startY: rubberBand?.startY || 0, endX: e.clientX, endY: e.clientY }
      const left = Math.min(rb.startX, rb.endX)
      const top = Math.min(rb.startY, rb.endY)
      const right = Math.max(rb.startX, rb.endX)
      const bottom = Math.max(rb.startY, rb.endY)
      const selected = new Set<string>()
      gridRef.current.querySelectorAll<HTMLElement>('[data-fb-name]').forEach(el => {
        const r = el.getBoundingClientRect()
        if (r.left < right && r.right > left && r.top < bottom && r.bottom > top) {
          const name = el.dataset.fbName
          if (name) selected.add(name)
        }
      })
      if (selected.size > 0) setSelectedFiles(selected)
    }
    const onMouseUp = () => {
      setRubberBand(null)
    }
    if (rubberBand) {
      window.addEventListener('mousemove', onMouseMove)
      window.addEventListener('mouseup', onMouseUp)
      return () => {
        window.removeEventListener('mousemove', onMouseMove)
        window.removeEventListener('mouseup', onMouseUp)
      }
    }
  }, [rubberBand])

  const handleGridMouseDown = (e: React.MouseEvent) => {
    // Only start rubber band on empty grid area (left button, not on file items)
    if (e.button !== 0) return
    const target = e.target as HTMLElement
    if (target.closest('[data-fb-name]')) return // clicked on a file item
    // Clear selection and start rubber band
    if (!e.ctrlKey && !e.shiftKey) {
      setSelectedFiles(new Set())
      setLastClickedFile(null)
    }
    setRubberBand({ startX: e.clientX, startY: e.clientY, endX: e.clientX, endY: e.clientY })
  }

  // Reset drag counter on drop
  const onDropWrapper = useCallback(async (e: React.DragEvent) => {
    dragCounterRef.current = 0
    await handleDropFiles(e)
  }, [handleDropFiles])

  // File Grid drop zone
  const gridDragHandlers = {
    onDragOver: handleDragOver,
    onDragEnter: handleDragEnter,
    onDragLeave: handleDragLeave,
    onDrop: onDropWrapper,
  }

  if (!sessionId) {
    return (
      <div className="file-browser fb-not-connected">
        <div className="fb-not-connected-msg">{t('files.notConnected')}</div>
      </div>
    )
  }

  return (
    <div
      ref={fileBrowserRef}
      className={`file-browser ${dropActive ? 'fb-drop-active' : ''}`}
      {...gridDragHandlers}
    >
      {/* Operation Log Floating Panel */}
      {operationLog && (
        <div className="fb-delete-log fb-archive-log">
          <div className="fb-delete-log-header">
            <span>📝 {t('files.operationLog')}</span>
            <button className="fb-delete-log-close" onClick={() => setOperationLog(null)}>✕</button>
          </div>
          <pre ref={operationLogRef} className="fb-delete-log-content">{operationLog.lines.join('\n')}</pre>
        </div>
      )}

      {/* Delete Log Floating Panel */}
      {deleteLog && (
        <div className="fb-delete-log">
          <div className="fb-delete-log-header">
            <span>🗑️ {t('files.deleteLog')}</span>
            <button className="fb-delete-log-close" onClick={() => setDeleteLog(null)}>✕</button>
          </div>
          <pre className="fb-delete-log-content">{deleteLog}</pre>
        </div>
      )}

      {/* Archive Progress Floating Panel */}
      {archiveProgress && (
        <div className="fb-delete-log fb-archive-log">
          <div className="fb-delete-log-header">
            <span>{archiveProgress.type === 'compress' ? `🗜️ ${t('files.compressing')}` : `📂 ${t('files.extracting')}`}{archiveProgress.done ? ` — ${t('files.done')}` : '...'}</span>
            <button className="fb-delete-log-close" onClick={() => setArchiveProgress(null)}>✕</button>
          </div>
          <pre ref={archiveLogRef} className="fb-delete-log-content">{archiveProgress.logs.join('\n')}</pre>
        </div>
      )}

      {/* Copy Progress Floating Panel */}
      {copyProgress && (
        <div className="fb-delete-log fb-archive-log">
          <div className="fb-delete-log-header">
            <span>📋 {t('files.copying')}{copyProgress.done ? ` — ${t('files.done')}` : '...'}</span>
            <button className="fb-delete-log-close" onClick={() => setCopyProgress(null)}>✕</button>
          </div>
          <pre ref={copyLogRef} className="fb-delete-log-content">{copyProgress.logs.join('\n')}</pre>
        </div>
      )}

      {/* Toolbar */}
      <div className="fb-toolbar">
        <button className="fb-btn" onClick={goUp} disabled={currentPath === '/'} title={t('files.goUp')}>
          <BackIcon />
        </button>
        <button className="fb-btn" onClick={() => navigateTo(currentPath, true)} title={t('files.refresh')}>
          <RefreshIcon />
        </button>

        <div className="fb-path-edit">
          <input
            ref={pathInputRef}
            className="fb-path-input"
            value={pathInputValue}
            onChange={(e) => setPathInputValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                handleGoToPath()
              } else if (e.key === 'Escape') {
                setPathInputValue(currentPath)
              }
            }}
            onFocus={() => setPathInputValue(currentPath)}
          />
          <button
            className="fb-btn fb-btn-go"
            onMouseDown={(e) => {
              e.preventDefault() // prevent input blur
              handleGoToPath()
            }}
            title={t('files.go')}
          >
            {t('files.go')} ➜
          </button>
          <div style={{ width: '1px', height: '20px', background: '#575b5cff', margin: '0 18px' }} />
          <div style={{ position: 'relative' }} ref={favoritesDropdownRef}>
            <button
              className="fb-btn fb-btn-favorites"
              onClick={() => setShowFavoritesDropdown(!showFavoritesDropdown)}
              title={t('files.favorites')}
            >
                   ⭐{t('files.favorites')} 
            </button>
            {showFavoritesDropdown && (
              <div className="fb-favorites-dropdown">
                {favorites.length === 0 ? (
                  <div className="fb-favorite-empty">
                    {t('files.noFavorites')}
                  </div>
                ) : (
                  favorites.map((path, idx) => (
                    <div
                      key={idx}
                      className="fb-favorite-item"
                      onClick={() => {
                        navigateTo(path)
                        setShowFavoritesDropdown(false)
                      }}
                    >
                      <span className="fb-favorite-path">{path}</span>
                      <button
                        className="fb-favorite-remove"
                        onClick={(e) => {
                          e.stopPropagation()
                          removeFavorite(path)
                        }}
                        title={t('files.removeFromFavorites')}
                      >
                        ✕
                      </button>
                    </div>
                  ))
                )}
              </div>
            )}
          </div>
        </div>

        <div className="fb-search-box">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="#656d76"><path d="M11.742 10.344a6.5 6.5 0 1 0-1.397 1.398h-.001l3.85 3.85a1 1 0 0 0 1.415-1.414l-3.85-3.85zm-5.242.656a5 5 0 1 1 0-10 5 5 0 0 1 0 10z"/></svg>
          <input
            className="fb-search-input"
            placeholder={t('files.searchPlaceholder')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          {searchQuery && (
            <button className="fb-search-clear" onClick={() => setSearchQuery('')}>✕</button>
          )}
        </div>
        <div className="fb-count">
          {searchQuery ? `${filteredFiles.length}/${files.length}` : files.length} {t('files.items')}
        </div>
      </div>

      {/* File Grid */}
      <div
        ref={gridRef}
        className={`fb-grid ${dropActive ? 'fb-grid-drop-active' : ''}`}
        onDoubleClick={(e) => {
          if (e.target === e.currentTarget) { navigateTo(currentPath) }
        }}
        onContextMenu={(e) => {
          // Check if right-click is on empty area (not on a file/folder item)
          const target = e.target as HTMLElement
          const isEmptyArea = 
            e.target === e.currentTarget ||
            target.classList.contains('fb-empty') ||
            target.closest('.fb-empty-container') !== null
          
          if (isEmptyArea) {
            e.preventDefault()
            setContextMenu(null)
            setBgContextMenu({ x: e.clientX, y: e.clientY })
          }
        }}
        onMouseDown={handleGridMouseDown}
      >
        {dropActive && <div className="fb-drop-overlay"> {t('files.dropFilesHere')}</div>}
        {loading && <div className="fb-loading">{t('common.loading')}</div>}
        {!loading && !jumpToPath && filteredFiles.length === 0 && !dropActive && (
          <div className="fb-empty-container">
            <div className="fb-empty">{searchQuery ? t('files.noSearchResults') : t('files.emptyDir')}</div>
            <button
              className="fb-btn fb-refresh-empty"
              onClick={() => navigateTo(currentPath, true)}
              title={t('files.refresh')}
            >
              <RefreshIcon /> {t('files.refresh')}
            </button>
          </div>
        )}
        {searchQuery && !loading && filteredFiles.length > 0 && (
          <div className="fb-search-hint">
            {t('files.searchResultsHint')}
            <button className="fb-search-hint-close" onClick={() => setSearchQuery('')}>✕</button>
          </div>
        )}
        {!loading && filteredFiles.map((entry) => (
          <div
            key={entry.name}
            className={`fb-item ${selectedFiles.has(entry.name) ? 'selected' : ''} ${draggingName === entry.name ? 'fb-item-dragging' : ''} ${clipboard?.paths.includes(currentPath === '/' ? `/${entry.name}` : `${currentPath}/${entry.name}`) && clipboard.mode === 'cut' ? 'fb-item-cut' : ''} ${dragOverTarget === entry.name ? (entry.isDir ? 'fb-item-dragover fb-item-dropinto' : 'fb-item-dragover') : ''}`}
            onClick={(e) => handleItemClick(entry, e)}
            onDoubleClick={() => handleItemDoubleClick(entry)}
            onContextMenu={(e) => handleItemContextMenu(e, entry)}
            onMouseDown={(e) => handleItemMouseDown(e, entry)}
            data-fb-name={entry.name}
            data-fb-isdir={entry.isDir ? 'true' : 'false'}
          >
            <div className="fb-icon">
              {entry.isDir ? <FolderIcon /> : entry.isSymlink ? <SymlinkIcon /> : <FileIcon />}
            </div>
            <div className="fb-name" title={entry.name}>{entry.name}</div>
          </div>
        ))}
      </div>

      {/* Drag ghost */}
      {dragGhost && (
        <div className="fb-drag-ghost" style={{ left: dragGhost.x, top: dragGhost.y }}>
          <span className="fb-drag-ghost-icon">{dragGhost.isDir ? '📁' : '📄'}</span>
          <span className="fb-drag-ghost-name">{dragGhost.name}{selectedFiles.size > 1 ? ` (+${selectedFiles.size - 1})` : ''}</span>
        </div>
      )}

      {/* Rubber band selection rect */}
      {rubberBand && (
        <div className="fb-rubber-band" style={{
          left: Math.min(rubberBand.startX, rubberBand.endX),
          top: Math.min(rubberBand.startY, rubberBand.endY),
          width: Math.abs(rubberBand.endX - rubberBand.startX),
          height: Math.abs(rubberBand.endY - rubberBand.startY),
        }} />
      )}

      {/* Status bar */}
      <div className="fb-status">
        <span>{currentPath}</span>
        {selectedFiles.size > 0 && <span className="fb-selected-info">
          {(() => {
            const sel = getSelectedEntries()
            if (sel.length === 0) return ''
            if (sel.length === 1) {
              const f = sel[0]
              return f.isDir ? `[dir] ${f.permissions}` : `${formatSize(f.size)}  ${f.permissions}  ${formatTime(f.mtime)}`
            }
            const totalSize = sel.reduce((s, f) => s + (f.isDir ? 0 : f.size), 0)
            return `${sel.length} ${t('files.selected')} (${formatSize(totalSize)})`
          })()}
        </span>}
      </div>

      {/* Editor modal */}
      {editor && (
        <div className={`fb-editor-overlay ${editor.minimized ? 'minimized' : ''}`} onClick={() => {
          if (editor.minimized) {
            setEditor({ ...editor, minimized: false })
            return
          }
          if (editor.content !== editor.originalContent) {
            showConfirm(t('files.unsavedChanges'), () => setEditor(null))
            return
          }
          setEditor(null)
        }}>
          <div className={`fb-editor ${editor.maximized ? 'maximized' : ''} ${editor.minimized ? 'minimized' : ''}`} onClick={(e) => e.stopPropagation()}>
            <div className="fb-editor-header">
              <span className="fb-editor-title">{editor.name} — {editor.path}</span>
              <div className="fb-editor-actions">
                <button
                  className="fb-editor-btn save"
                  onClick={handleSaveFile}
                  disabled={editor.saving || editor.content === editor.originalContent}
                >
                  {editor.saving ? t('common.saving') : `💾 ${t('common.save')}`}
                </button>
                <button className="fb-editor-btn minimize" onClick={() => setEditor({ ...editor, minimized: true })} title={t('files.minimize')}>—</button>
                <button className="fb-editor-btn maximize" onClick={() => setEditor({ ...editor, maximized: !editor.maximized })} title={editor.maximized ? t('files.restore') : t('files.maximize')}>
                  {editor.maximized ? '❐' : '▢'}
                </button>
                <button className="fb-editor-btn close" onClick={() => {
                  if (editor.content !== editor.originalContent) {
                    showConfirm(t('files.unsavedChanges'), () => setEditor(null))
                    return
                  }
                  setEditor(null)
                }}>✕</button>
              </div>
            </div>
            {!editor.minimized && (
              <textarea
                className="fb-editor-content"
                value={editor.content}
                onChange={(e) => setEditor({ ...editor, content: e.target.value })}
                spellCheck={false}
                onKeyDown={(e) => {
                  if (e.ctrlKey && e.key === 's') {
                    e.preventDefault()
                    handleSaveFile()
                  }
                }}
              />
            )}
          </div>
        </div>
      )}

      {/* Context Menu */}
      {contextMenu && (
        <div
          ref={menuRef}
          className="fb-context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          {contextMenu.entry.isDir ? (
            <>
            <div className="fb-context-item" onClick={() => {
              const newPath = currentPath === '/' ? `/${contextMenu.entry.name}` : `${currentPath}/${contextMenu.entry.name}`
              navigateTo(newPath)
              setContextMenu(null)
            }}>
              📂 {t('files.open')}
            </div>
            </>
          ) : (
            <div className="fb-context-item" onClick={() => {
              openEditor(contextMenu.entry)
              setContextMenu(null)
            }}>
              ✏️ {t('files.edit')}
            </div>
          )}
          <div className="fb-context-divider" />
          <div className="fb-context-item" onClick={() => {
            handleCopy()
            setContextMenu(null)
          }}>
            📋 {t('common.copy')}{selectedFiles.size > 1 ? ` (${selectedFiles.size})` : ''}
          </div>
          <div className="fb-context-item" onClick={() => {
            handleCut()
            setContextMenu(null)
          }}>
            ✂️ {t('common.cut')}{selectedFiles.size > 1 ? ` (${selectedFiles.size})` : ''}
          </div>
          {clipboard && (
            <div className="fb-context-item" onClick={() => {
              handlePaste()
              setContextMenu(null)
            }}>
              📎 {t('common.paste')}
            </div>
          )}
          <div className="fb-context-divider" />
          <div className="fb-context-item" onClick={() => {
            handleRename(contextMenu.entry)
            setContextMenu(null)
          }}>
            ✏️ {t('common.rename')}
          </div>
          {!contextMenu.entry.isDir && (
            <div className="fb-context-item" onClick={() => {
              handleSaveAs(contextMenu.entry)
              setContextMenu(null)
            }}>
              💾 {t('files.saveAs')}
            </div>
          )}
          <div className="fb-context-item" onClick={() => {
            setCompressDialog({ names: selectedFiles.has(contextMenu.entry.name) && selectedFiles.size > 1 ? Array.from(selectedFiles) : [contextMenu.entry.name] })
            setCompressFormat('zip')
            setContextMenu(null)
          }}>
            🗜️ {t('files.compress')}{selectedFiles.has(contextMenu.entry.name) && selectedFiles.size > 1 ? ` (${selectedFiles.size})` : ''}
          </div>
          {isArchive(contextMenu.entry.name) && (
            <div className="fb-context-item" onClick={() => {
              handleExtract(contextMenu.entry)
              setContextMenu(null)
            }}>
              📂 {t('files.extract')}
            </div>
          )}
          <div className="fb-context-item" onClick={() => {
            handleShowInfo(contextMenu.entry)
            setContextMenu(null)
          }}>
            ℹ️ {t('files.fileInfo')}
          </div>
          <div className="fb-context-item" onClick={() => {
            handleOpenPermissions(contextMenu.entry)
            setContextMenu(null)
          }}>
            🔒 {t('files.setPermissions')}
          </div>
          <div className="fb-context-divider" />
          <div className="fb-context-item danger" onClick={() => {
            handleDelete()
            setContextMenu(null)
          }}>
            🗑️ {t('common.delete')}{selectedFiles.size > 1 ? ` (${selectedFiles.size})` : ''}
          </div>
        </div>
      )}

      {/* Background Context Menu (right-click on empty area) */}
      {bgContextMenu && (
        <div
          ref={bgMenuRef}
          className="fb-context-menu"
          style={{ left: bgContextMenu.x, top: bgContextMenu.y }}
        >
          <div className="fb-context-item" onClick={() => {
            navigateTo(currentPath, true)
            setBgContextMenu(null)
          }}>
            🔄 {t('files.refresh')}
          </div>
          <div className="fb-context-divider" />
          <div className="fb-context-item" onClick={() => {
            handleNewFile()
            setBgContextMenu(null)
          }}>
            📄 {t('files.newFile')}
          </div>
          <div className="fb-context-item" onClick={() => {
            handleNewFolder()
            setBgContextMenu(null)
          }}>
            📁 {t('files.newFolder')}
          </div>
          <div className="fb-context-item" onClick={() => {
            onCdHere?.(currentPath)
            setBgContextMenu(null)
          }}>
             💻 {t('files.cdHere')}
          </div>
          <div className="fb-context-divider" />
          <div className="fb-context-item" onClick={() => {
            addFavorite(currentPath)
            setBgContextMenu(null)
          }}>
            ⭐ {isFavorite(currentPath) ? t('files.removeFromFavorites') : t('files.addToFavorites')}
          </div>
          {clipboard && (
            <>
              <div className="fb-context-divider" />
              <div className="fb-context-item" onClick={() => {
                handlePaste()
                setBgContextMenu(null)
              }}>
                📎 {t('common.paste')}
              </div>
            </>
          )}
          <div className="fb-context-divider" />
          <div className="fb-context-item" onClick={() => {
            setDownloadDialog({ url: '' })
            setBgContextMenu(null)
          }}>
            ⬇️ {t('common.download')}
          </div>
          {selectedFiles.size > 0 && (
            <div className="fb-context-item" onClick={() => {
              setCompressDialog({ names: Array.from(selectedFiles) })
              setCompressFormat('zip')
              setBgContextMenu(null)
            }}>
              🗜️ {t('files.compress')} ({selectedFiles.size})
            </div>
          )}
        </div>
      )}

      {/* File Info Dialog */}
      {fileInfo && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-info-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setFileInfo(null)}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">{t('files.fileInfo')}</div>
            <div className="fb-info-grid">
              <div className="fb-info-label">{t('common.name')}</div>
              <div className="fb-info-value">{fileInfo.entry.name}</div>
              <div className="fb-info-label">{t('common.type')}</div>
              <div className="fb-info-value">
                {fileInfo.entry.isDir ? t('files.folder') : fileInfo.entry.isSymlink ? t('files.symbolicLink') : `${t('files.file')} (.${getFileExtension(fileInfo.entry.name) || 'unknown'})`}
              </div>
              <div className="fb-info-label">{t('common.size')}</div>
              <div className="fb-info-value">{fileInfo.entry.isDir ? '—' : formatSize(fileInfo.entry.size)}</div>
              <div className="fb-info-label">{t('files.permissions')}</div>
              <div className="fb-info-value">{fileInfo.entry.permissions || '—'}</div>
              <div className="fb-info-label">{t('files.owner')}</div>
              <div className="fb-info-value">{fileInfo.entry.owner || '—'}</div>
              <div className="fb-info-label">{t('files.modified')}</div>
              <div className="fb-info-value">{formatTime(fileInfo.entry.mtime)}</div>
              <div className="fb-info-label">{t('common.path')}</div>
              <div className="fb-info-value fb-info-path">{fileInfo.path}</div>
            </div>
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn primary" onClick={() => setFileInfo(null)}>{t('common.close')}</button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm Dialog */}
      {confirmDialog && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setConfirmDialog(null)}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">{t('files.confirmTitle')}</div>
            <div className="fb-confirm-msg">{confirmDialog.message}</div>
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn" onClick={() => setConfirmDialog(null)}>{t('common.cancel')}</button>
              <button className="fb-dialog-btn danger" onClick={() => {
                confirmDialog.onConfirm()
                setConfirmDialog(null)
              }}>{t('common.confirm')}</button>
            </div>
          </div>
        </div>
      )}

      {/* Conflict Dialog */}
      {conflictDialog && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-conflict-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => {
                conflictDialog.resolve({ action: 'skip', applyToAll: false })
                setConflictDialog(null)
              }}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">{t('files.fileConflict')}</div>
            <div className="fb-conflict-msg">
              <span className="fb-conflict-icon">{conflictDialog.item.isDir ? '📁' : '📄'}</span>
              {t('files.alreadyExists', { name: conflictDialog.item.name, type: conflictDialog.item.isDir ? t('files.folder') : t('files.file') })}
            </div>
            <div className="fb-conflict-question">{t('files.whatToDo')}</div>
            {conflictDialog.remaining > 0 && (
              <label className="fb-conflict-apply-all">
                <input type="checkbox" id="conflict-apply-all" />
                <span>{t('files.applyToAll', { count: conflictDialog.remaining + 1 })}</span>
              </label>
            )}
            <div className="fb-conflict-actions">
              <button
                className="fb-dialog-btn fb-conflict-replace"
                onClick={() => {
                  const applyAll = (document.getElementById('conflict-apply-all') as HTMLInputElement)?.checked ?? false
                  conflictDialog.resolve({ action: 'replace', applyToAll: applyAll })
                  setConflictDialog(null)
                }}
              >
                {t('files.replace')}
              </button>
              <button
                className="fb-dialog-btn primary"
                onClick={() => {
                  const applyAll = (document.getElementById('conflict-apply-all') as HTMLInputElement)?.checked ?? false
                  conflictDialog.resolve({ action: 'rename', applyToAll: applyAll })
                  setConflictDialog(null)
                }}
              >
                {t('common.rename')}
              </button>
              <button
                className="fb-dialog-btn"
                onClick={() => {
                  const applyAll = (document.getElementById('conflict-apply-all') as HTMLInputElement)?.checked ?? false
                  conflictDialog.resolve({ action: 'skip', applyToAll: applyAll })
                  setConflictDialog(null)
                }}
              >
                {t('files.skip')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Prompt Dialog */}
      {promptDialog && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-prompt-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setPromptDialog(null)}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">{promptDialog.title}</div>
            <input
              ref={promptInputRef}
              className="fb-prompt-input"
              value={promptDialog.value}
              onChange={(e) => setPromptDialog({ ...promptDialog, value: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  promptDialog.onSubmit(promptDialog.value)
                  setPromptDialog(null)
                }
                if (e.key === 'Escape') setPromptDialog(null)
              }}
              autoFocus
            />
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn" onClick={() => setPromptDialog(null)}>{t('common.cancel')}</button>
              <button className="fb-dialog-btn primary" onClick={() => {
                promptDialog.onSubmit(promptDialog.value)
                setPromptDialog(null)
              }}>{t('common.ok')}</button>
            </div>
          </div>
        </div>
      )}

      {/* Permission Dialog */}
      {permissionDialog && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-perm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setPermissionDialog(null)}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">{t('files.setPermissionsTitle')}</div>
            <div className="fb-perm-info">
              <span className="fb-perm-name">
                {permissionDialog.names.length === 1 
                  ? permissionDialog.names[0]
                  : `${t('files.selectedItems', { count: permissionDialog.names.length })}`}
              </span>
              <span className="fb-perm-current">{t('files.current')}: {permissionDialog.currentPerms}</span>
            </div>
            <div className="fb-perm-quick">
              {[['644','rw-r--r--'],['755','rwxr-xr-x'],['600','rw-------'],['700','rwx------'],['777','rwxrwxrwx'],['400','r--------']].map(([mode, label]) => (
                <button
                  key={mode}
                  className={`fb-perm-chip ${permissionDialog.mode === mode ? 'active' : ''}`}
                  onClick={() => setPermissionDialog({ ...permissionDialog, mode })}
                >
                  {mode}<span className="fb-perm-chip-label">{label}</span>
                </button>
              ))}
            </div>
            <input
              className="fb-prompt-input"
              placeholder={t('files.enterMode')}
              value={permissionDialog.mode}
              onChange={(e) => setPermissionDialog({ ...permissionDialog, mode: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleApplyPermissions()
                if (e.key === 'Escape') setPermissionDialog(null)
              }}
            />
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn" onClick={() => setPermissionDialog(null)}>{t('common.cancel')}</button>
              <button className="fb-dialog-btn primary" onClick={handleApplyPermissions} disabled={!permissionDialog.mode}>{t('common.apply')}</button>
            </div>
          </div>
        </div>
      )}

      {/* Compress Dialog */}
      {compressDialog && (
        <div className="fb-dialog-overlay">
          <div className="fb-dialog fb-compress-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setCompressDialog(null)}
              title="关闭"
            >×</button>
            <div className="fb-dialog-title">🗜️ {t('files.compressTitle', { count: compressDialog.names.length })}</div>
            <div className="fb-compress-items">
              {compressDialog.names.slice(0, 5).map(n => (
                <span key={n} className="fb-compress-item">{n}</span>
              ))}
              {compressDialog.names.length > 5 && <span className="fb-compress-more">{t('files.moreItems', { count: compressDialog.names.length - 5 })}</span>}
            </div>
            <div style={{ display: 'flex', gap: '6px', marginBottom: '12px' }}>
              <span style={{ fontSize: '12px', color: '#8b949e', alignSelf: 'center' }}>{t('files.compressFormat')}:</span>
              {(['zip', 'tar.gz', 'tar.bz2'] as const).map(fmt => (
                <button
                  key={fmt}
                  className={`fb-dialog-btn ${compressFormat === fmt ? 'primary' : ''}`}
                  style={{ flex: 1, fontSize: '12px', padding: '6px 0' }}
                  onClick={() => {
                    setCompressFormat(fmt)
                    const input = document.getElementById('compress-name-input') as HTMLInputElement
                    if (input) input.value = input.value.replace(/\.(zip|tar\.gz|tar\.bz2)$/, `.${fmt}`)
                  }}
                >.{fmt}</button>
              ))}
            </div>
            <input
              className="fb-prompt-input"
              placeholder={t('files.archiveName')}
              defaultValue={(compressDialog.names.length === 1 ? compressDialog.names[0] : 'archive') + '.zip'}
              onKeyDown={(e) => {
                const val = (e.target as HTMLInputElement).value
                if (e.key === 'Enter' && val.trim()) handleCompress(compressDialog.names, val.trim(), compressFormat)
                if (e.key === 'Escape') setCompressDialog(null)
              }}
              autoFocus
              id="compress-name-input"
            />
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn" onClick={() => setCompressDialog(null)}>{t('common.cancel')}</button>
              <button className="fb-dialog-btn primary" onClick={() => {
                const input = document.getElementById('compress-name-input') as HTMLInputElement
                if (input?.value.trim()) handleCompress(compressDialog.names, input.value.trim(), compressFormat)
              }}>{t('files.compress')} (.{compressFormat})</button>
            </div>
          </div>
        </div>
      )}

      {/* Download Dialog */}
      {downloadDialog && (
        <div className="fb-dialog-overlay" onClick={() => {
          if (downloadProgress && downloadProgress.status === 'downloading') return
          setDownloadDialog(null)
        }}>
          <div className="fb-dialog fb-download-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="fb-dialog-title">⬇️ {t('files.downloadFromUrl')}</div>
            <input
              className="fb-prompt-input"
              placeholder="https://example.com/file.zip"
              value={downloadDialog.url}
              onChange={(e) => setDownloadDialog({ ...downloadDialog, url: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleDownload()
                if (e.key === 'Escape' && (!downloadProgress || downloadProgress.status !== 'downloading')) setDownloadDialog(null)
              }}
              disabled={!!downloadProgress}
              autoFocus
            />
            <div className="fb-download-path">
              {t('files.saveTo')} {currentPath === '/' ? '/' : `${currentPath}/`}
            </div>
            <div className="fb-dialog-actions">
              <button className="fb-dialog-btn" onClick={() => setDownloadDialog(null)} disabled={!!downloadProgress}>{t('common.cancel')}</button>
              <button className="fb-dialog-btn primary" onClick={handleDownload} disabled={!downloadDialog.url || !!downloadProgress}>
                {downloadProgress ? t('files.downloading') : t('common.download')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Download Progress Bar (bottom of file browser) */}
      {downloadProgress && (
        <div className="fb-progress-bar">
          <div className="fb-progress-track">
            <div
              className={`fb-progress-fill ${downloadProgress.status === 'error' ? 'error' : downloadProgress.status === 'done' ? 'done' : ''}`}
              style={{ width: `${downloadProgress.progress}%` }}
            />
          </div>
          <span className="fb-progress-text">
            {downloadProgress.status === 'starting' ? t('files.starting') :
             downloadProgress.status === 'done' ? t('files.complete') :
             downloadProgress.status === 'error' ? t('files.failed') :
             `${Math.round(downloadProgress.progress)}%`}
          </span>
        </div>
      )}

      {/* Upload menu */}
      {uploadMenuOpen && (
        <div className="fb-upload-menu">
          <div className="fb-upload-menu-item" onClick={() => { handleUploadFilesBtn(); setUploadMenuOpen(false) }}>
            📄 {t('files.uploadFiles')}
          </div>
          <div className="fb-upload-menu-item" onClick={() => { handleUploadFolder(); setUploadMenuOpen(false) }}>
            📁 {t('files.uploadFolder')}
          </div>
        </div>
      )}

      {/* Upload floating button */}
      <button
        className="fb-upload-btn"
        onClick={(e) => { e.stopPropagation(); setUploadMenuOpen(v => !v) }}
        title={t('common.upload')}
      >+</button>

      {/* Status Bar */}
      <div className="fb-status-bar">
        <span className="fb-status-path">{currentPath}</span>
        <span className="fb-status-cache">
          {cacheTime ? `🕐 ${new Date(cacheTime).toLocaleString()}` : '—'}
        </span>
      </div>
    </div>
  )
})

