import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import Dashboard from './panels/Dashboard'
import InstallLnmp from './panels/InstallLnmp'
import NginxPanel from './panels/NginxPanel'
// ponytail: PhpPanel not yet wired up
// import PhpPanel from './panels/PhpPanel'
import SitesPanel from './panels/SitesPanel'
import SslPanel from './panels/SslPanel'
import MonitorPanel from './panels/MonitorPanel'
import FirewallPanel from './panels/FirewallPanel'
import SoftwareRepo from './panels/SoftwareRepo'
import ServerSettingsPanel from './panels/ServerSettingsPanel'
import SiteLogsPanel from './panels/SiteLogsPanel'
import BbrPanel from './panels/BbrPanel'
import DatabasePanel from './panels/DatabasePanel'
import RedisPanel from './panels/RedisPanel'
// ponytail: DockerPanel not yet wired up
// import DockerPanel from './panels/DockerPanel'
import Terminal from './Terminal'
import type { TerminalHandle } from './Terminal'
import FileBrowser, { type FileBrowserHandle } from './FileBrowser'

type PanelSection = 'dashboard' | 'terminal' | 'files' | 'install' | 'software' | 'nginx' | 'php' | 'sites' | 'logs' | 'ssl' | 'monitor' | 'firewall' | 'bbr' | 'docker' | 'database' | 'redis' | 'settings'

interface AppSettings {
  auto_reconnect: boolean
  reconnect_interval: number
  max_reconnect_attempts: number
  cache_ttl_hours: number
  cache_max_files: number
  cache_enabled: boolean
}

interface ServerPanelProps {
  sessionId: string | null
  connHost?: string
  jumpToPath?: string | null
  setJumpToPath?: (path: string | null) => void
  termRef?: React.RefObject<TerminalHandle | null>
  onStartUpload?: (files: { file: File; fileName: string; remotePath: string }[]) => void
  onUploadComplete?: React.MutableRefObject<(() => void) | null>
  appSettings?: AppSettings
  onToggleAutoReconnect?: () => void
  onUpdateSettings?: (settings: Partial<AppSettings>) => Promise<void>
  onReconnect?: () => void
}

const NAV_ITEMS: { key: PanelSection; label: string; icon: string }[] = [
  { key: 'dashboard', label: 'Dashboard', icon: '📊' },
  { key: 'terminal', label: 'Terminal', icon: '💻' },
  { key: 'files', label: 'Files', icon: '📂' },
  { key: 'install', label: 'Install LNMP', icon: '📦' },
  { key: 'software', label: 'Software', icon: '🧩' },
  // { key: 'nginx', label: 'Nginx', icon: '' },
  { key: 'database', label: 'DB/SQL管理', icon: '🗄' },
  { key: 'redis', label: 'Redis', icon: '⚡' },
  // { key: 'php', label: 'PHP', icon: '' },
  { key: 'sites', label: 'Sites', icon: '🌐' },
  { key: 'logs', label: 'Logs', icon: '📋' },
  { key: 'ssl', label: 'SSL', icon: '🔒' },
  { key: 'monitor', label: 'Monitor', icon: '📈' },
  { key: 'firewall', label: 'Firewall', icon: '🧱' },
  { key: 'bbr', label: 'BBR Acceleration', icon: '🚀' },
  // { key: 'docker', label: 'Docker', icon: '' },
  { key: 'settings', label: 'Settings', icon: '⚙' },
]

export default function ServerPanel({ sessionId, connHost, jumpToPath, setJumpToPath, termRef, onStartUpload, onUploadComplete, appSettings, onToggleAutoReconnect, onUpdateSettings, onReconnect }: ServerPanelProps) {
  const [activeSection, setActiveSectionRaw] = useState<PanelSection>('dashboard')
  const cdHereRef = useRef<string | null>(null)
  const fileBrowserRef = useRef<FileBrowserHandle | null>(null)

  // Load last panel from SQLite
  useEffect(() => {
    invoke<string>('ui_state_get', { key: 'lastPanel' })
      .then(v => { if (v && NAV_ITEMS.some(s => s.key === v)) setActiveSectionRaw(v as PanelSection) })
      .catch(() => {})
  }, [])

  const setActiveSection = (key: PanelSection) => {
    setActiveSectionRaw(key)
    invoke('ui_state_set', { key: 'lastPanel', value: key }).catch(() => {})
  }

  const handleNavigate = (section: string) => {
    setActiveSection(section as PanelSection)
  }

  // ponytail: removed auto-switch to terminal on connection - let user choose where to go

  // Clear jumpToPath after FileBrowser consumes it
  useEffect(() => {
    if (jumpToPath && activeSection === 'files') {
      const timer = setTimeout(() => setJumpToPath?.(null), 100)
      return () => clearTimeout(timer)
    }
  }, [jumpToPath, activeSection]) // eslint-disable-line

  // Handle cd-here from FileBrowser
  useEffect(() => {
    if (activeSection === 'terminal' && cdHereRef.current) {
      const path = cdHereRef.current
      cdHereRef.current = null
      setTimeout(() => termRef?.current?.sendCommand(`cd '${path}'`), 200)
    }
  }, [activeSection]) // eslint-disable-line

  const handleInternalOpenFolder = (path: string) => {
    setJumpToPath?.(path)
    setActiveSection('files')
  }

  const handleCdHere = (path: string) => {
    cdHereRef.current = path
    setActiveSection('terminal')
  }

  // Handle upload complete - refresh current directory
  const handleUploadComplete = useCallback(() => {
    if (fileBrowserRef.current && activeSection === 'files') {
      fileBrowserRef.current.refreshCurrentDirectory()
    }
  }, [activeSection])

  useEffect(() => {
    if (onUploadComplete) onUploadComplete.current = handleUploadComplete
  }, [onUploadComplete, handleUploadComplete])

  const renderContent = () => {
    switch (activeSection) {
      case 'dashboard':
        return <Dashboard sessionId={sessionId} onNavigate={handleNavigate} />
      case 'install':
        return <InstallLnmp sessionId={sessionId} onInstallationComplete={onReconnect} />
      case 'nginx':
        return <NginxPanel sessionId={sessionId} />
      // case 'php':
      //   return <PhpPanel sessionId={sessionId} />
      case 'logs':
        return <SiteLogsPanel sessionId={sessionId} />
      case 'ssl':
        return <SslPanel sessionId={sessionId} />
      case 'monitor':
        return <MonitorPanel sessionId={sessionId} />
      case 'firewall':
        return <FirewallPanel sessionId={sessionId} />
      case 'software':
        return <SoftwareRepo sessionId={sessionId} onDisconnect={onReconnect} />
      case 'bbr':
        return <BbrPanel sessionId={sessionId} />
      case 'database':
        return <DatabasePanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      case 'redis':
        return <RedisPanel sessionId={sessionId} onNavigateToSoftware={() => setActiveSection('software')} />
      // case 'docker':
      //   return <DockerPanel sessionId={sessionId} />
      case 'settings':
        return <ServerSettingsPanel sessionId={sessionId} onNavigate={handleNavigate} appSettings={appSettings} onToggleAutoReconnect={onToggleAutoReconnect} onUpdateSettings={onUpdateSettings} />
      default:
        return null
    }
  }

  return (
    <div className="server-panel">
      <nav className="sp-nav">
        {NAV_ITEMS.map(item => (
          <button
            key={item.key}
            className={`sp-nav-item ${activeSection === item.key ? 'active' : ''}`}
            onClick={() => setActiveSection(item.key)}
            disabled={!sessionId && item.key !== 'dashboard'}
          >
            <span className="sp-nav-icon">{item.icon}</span>
            <span className="sp-nav-label">{item.label}</span>
          </button>
        ))}
      </nav>
      <div className="sp-content">
        {/* Terminal always mounted to preserve SSH session */}
        <div style={{ display: activeSection === 'terminal' ? 'block' : 'none', height: '100%' }}>
          <Terminal ref={termRef} sessionId={sessionId} isActive={activeSection === 'terminal'} />
        </div>
        {/* Files always mounted to preserve state and avoid reload flash */}
        <div style={{ display: activeSection === 'files' ? 'block' : 'none', height: '100%' }}>
          <FileBrowser sessionId={sessionId} connHost={connHost} jumpToPath={jumpToPath} ref={fileBrowserRef} onTerminalCommand={termRef?.current ? (cmd: string) => termRef.current?.sendCommand(cmd) : undefined} onCdHere={handleCdHere} onStartUpload={onStartUpload} />
        </div>
        {/* Sites always mounted to preserve list state */}
        <div style={{ display: activeSection === 'sites' ? 'block' : 'none', height: '100%' }}>
          <SitesPanel sessionId={sessionId} onOpenFolder={handleInternalOpenFolder} onNavigateToInstall={() => setActiveSection('install')} />
        </div>
        {activeSection !== 'terminal' && activeSection !== 'files' && activeSection !== 'sites' && renderContent()}
      </div>
    </div>
  )
}
