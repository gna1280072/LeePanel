import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'
import ServiceUnavailable from './ServiceUnavailable'

interface DbInfo {
  name: string
  user?: string // Actual MySQL user name (may differ from db name)
  size_mb: number
  password?: string // Optional: loaded from localStorage
  access_type?: 'local' | 'any' | 'ip' // Access permission type
  allowed_ip?: string // Allowed IP when access_type is 'ip'
}

interface BackupInfo {
  filename: string
  size_bytes: number
  created_at: string
}

interface DbCredential {
  db_name: string
  db_user: string
  password: string
  access_type: string
  allowed_ip: string
}

interface DatabasePanelProps {
  sessionId: string | null
  onNavigateToSoftware?: () => void
}

export default function DatabasePanel({ sessionId, onNavigateToSoftware }: DatabasePanelProps) {
  const { t } = useTranslation()
  const [databases, setDatabases] = useState<DbInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [msg, setMsg] = useState('')
  
  // Search
  const [searchQuery, setSearchQuery] = useState('')
  
  // Pagination
  const [currentPage, setCurrentPage] = useState(1)
  const [pageSize, setPageSize] = useState(10)
  
  // Create dialog
  const [showCreateDialog, setShowCreateDialog] = useState(false)
  const [newDbName, setNewDbName] = useState('')
  const [newDbUser, setNewDbUser] = useState('')
  const [newDbPass, setNewDbPass] = useState('')
  const [savePasswordLocally, setSavePasswordLocally] = useState(true) // Default: save password
  const [dbCharset, setDbCharset] = useState('utf8mb4') // Default charset
  const [accessType, setAccessType] = useState<'local' | 'any' | 'ip'>('local') // Default: local server
  const [allowedIp, setAllowedIp] = useState('') // For custom IP access
  const [creating, setCreating] = useState(false)
  
  // Delete dialog
  const [deleteTarget, setDeleteTarget] = useState<DbInfo | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [deleteConfirmName, setDeleteConfirmName] = useState('')
  
  // Clear database dialog
  const [clearTarget, setClearTarget] = useState<DbInfo | null>(null)
  const [clearing, setClearing] = useState(false)
  const [clearConfirmText, setClearConfirmText] = useState('')
  
  // Password visibility
  const [visiblePasswords, setVisiblePasswords] = useState<Set<string>>(new Set())
  
  // Selected databases for batch operations
  const [selectedDbs, setSelectedDbs] = useState<Set<string>>(new Set())
  
  // Database remarks (key: dbName, value: remark)
  const [dbRemarks, setDbRemarks] = useState<Record<string, string>>({})
  const [editingRemark, setEditingRemark] = useState<string | null>(null)
  const [remarkInput, setRemarkInput] = useState('')
  
  // Change root password dialog
  const [showChangePwDialog, setShowChangePwDialog] = useState(false)
  const [newRootPassword, setNewRootPassword] = useState('')
  const [showRootPassword, setShowRootPassword] = useState(false)
  const [changingPw, setChangingPw] = useState(false)
  
  // Change access permission dialog
  const [showAccessDialog, setShowAccessDialog] = useState(false)
  const [accessTarget, setAccessTarget] = useState<{ name: string; user: string } | null>(null)
  const [newAccessType, setNewAccessType] = useState<'local' | 'any' | 'ip'>('local')
  const [newAllowedIp, setNewAllowedIp] = useState('')
  const [changingAccess, setChangingAccess] = useState(false)
  
  // Change db user password dialog
  const [showChangePwDbDialog, setShowChangePwDbDialog] = useState(false)
  const [changePwTarget, setChangePwTarget] = useState<{ name: string; user: string } | null>(null)
  const [newDbPassword, setNewDbPassword] = useState('')
  const [changingDbPw, setChangingDbPw] = useState(false)
  const [updateLocalPassword, setUpdateLocalPassword] = useState(true)
  
  // Backup dialog
  const [showBackupDialog, setShowBackupDialog] = useState(false)
  const [backupTarget, setBackupTarget] = useState<string | null>(null)
  const [backups, setBackups] = useState<BackupInfo[]>([])
  const [backingUp, setBackingUp] = useState(false)
  const [loadingBackups, setLoadingBackups] = useState(false)
  
  // Import dialog
  const [showImportDialog, setShowImportDialog] = useState(false)
  const [importTarget, setImportTarget] = useState<string | null>(null)
  const [importMode, setImportMode] = useState<'upload' | 'backup'>('upload')
  const [selectedFile, setSelectedFile] = useState<File | null>(null)
  const [selectedBackup, setSelectedBackup] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const [importBackups, setImportBackups] = useState<BackupInfo[]>([])
  const [loadingImportBackups, setLoadingImportBackups] = useState(false)
  const [missingToolModal, setMissingToolModal] = useState(false)
  
  // Database credentials from SQLite (key: dbName)
  const [dbCredentials, setDbCredentials] = useState<Record<string, DbCredential>>({})
  
  const fetchDatabases = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    try {
      const list = await invoke<DbInfo[]>('server_list_databases', { sessionId })
      
      // Load credentials from SQLite
      try {
        const credsList = await invoke<DbCredential[]>('server_get_db_credentials', { sessionId })
        const credsMap: Record<string, DbCredential> = {}
        for (const cred of credsList) {
          credsMap[cred.db_name] = cred
        }
        setDbCredentials(credsMap)
        
        // Merge credentials into database list
        const withCredentials = list.map(db => {
          const cred = credsMap[db.name]
          return cred ? {
            ...db,
            user: cred.db_user || db.name, // Use saved db_user, fallback to db name
            password: cred.password || undefined,
            access_type: (cred.access_type as 'local' | 'any' | 'ip') || 'local',
            allowed_ip: cred.allowed_ip || undefined,
          } : { ...db, user: db.name } // Default: user = db name
        })
        setDatabases(withCredentials)
      } catch (e) {
        console.error('Failed to load db credentials:', e)
        setDatabases(list)
      }
      
      // Load remarks from SQLite
      try {
        const remarksList = await invoke<[string, string][]>('server_get_db_remarks', { sessionId })
        const remarksMap: Record<string, string> = {}
        for (const [dbName, remark] of remarksList) {
          remarksMap[dbName] = remark
        }
        setDbRemarks(remarksMap)
      } catch (e) {
        console.error('Failed to load db remarks:', e)
      }
      
      setError('') // Clear error on success
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])
  
  useEffect(() => { fetchDatabases() }, [fetchDatabases])
  
  // ponytail: passwords now loaded inline in fetchDatabases, no separate effect needed

  // Filter databases based on search query
  const filteredDatabases = databases.filter(db => 
    db.name.toLowerCase().includes(searchQuery.toLowerCase())
  )
  
  // Calculate pagination
  const totalPages = Math.ceil(filteredDatabases.length / pageSize)
  const startIndex = (currentPage - 1) * pageSize
  const paginatedDatabases = filteredDatabases.slice(startIndex, startIndex + pageSize)
  
  // Reset to page 1 when search changes
  useEffect(() => {
    setCurrentPage(1)
  }, [searchQuery])
  
  const handleCreateDatabase = async () => {
    if (!newDbName.trim() || !newDbUser.trim() || !newDbPass.trim()) {
      setMsg(t('database.fillAllFields'))
      return
    }
    
    // Validate IP if access type is 'ip'
    if (accessType === 'ip') {
      const ips = allowedIp.split('\n').map(line => line.trim()).filter(line => line.length > 0)
      if (ips.length === 0) {
        setMsg(t('database.enterAllowedIp'))
        return
      }
      // Basic validation: check for empty lines or invalid characters
      const invalidLines = ips.filter(ip => !/^([0-9]{1,3}\.){3}[0-9]{1,3}(\/[0-9]{1,2})?$/.test(ip) && ip !== '%')
      if (invalidLines.length > 0) {
        setMsg(`Invalid IP format: ${invalidLines.join(', ')}`)
        return
      }
    }
    
    setCreating(true)
    setMsg('')
    try {
      const result = await invoke<string>('server_mysql_create_database', {
        sessionId,
        dbName: newDbName.trim(),
        dbUser: newDbUser.trim(),
        dbPass: newDbPass.trim(),
        charset: dbCharset,
        accessType: accessType,
        allowedIp: accessType === 'ip' ? allowedIp.trim() : ''
      })
      setMsg(result)
      
      // Save credentials to SQLite
      try {
        await invoke<string>('server_save_db_credentials', {
          sessionId,
          dbName: newDbName.trim(),
          dbUser: newDbUser.trim(),
          password: savePasswordLocally ? newDbPass : '',
          accessType,
          allowedIp: accessType === 'ip' ? allowedIp.trim() : ''
        })
        if (savePasswordLocally) {
          setMsg(result + ' (Password saved)')
        }
      } catch (e) {
        console.error('Failed to save credentials:', e)
      }
      
      setShowCreateDialog(false)
      setNewDbName('')
      setNewDbUser('')
      setNewDbPass('')
      setSavePasswordLocally(true) // Reset to default
      setDbCharset('utf8mb4') // Reset to default
      setAccessType('local') // Reset to default
      setAllowedIp('')
      await fetchDatabases()
    } catch (e) {
      setMsg(`${t('common.error')}: ${String(e)}`)
    } finally {
      setCreating(false)
    }
  }
  
  const handleDeleteDatabase = async () => {
    if (!deleteTarget) return
    
    setDeleting(true)
    setMsg('')
    try {
      const result = await invoke<string>('server_mysql_delete_database', {
        sessionId,
        dbName: deleteTarget.name,
        dbUser: deleteTarget.user || deleteTarget.name // Use actual user, fallback to db name
      })
      setMsg(result)
      setDeleteTarget(null)
      await fetchDatabases()
    } catch (e) {
      setMsg(`${t('common.delete')}: ${String(e)}`)
    } finally {
      setDeleting(false)
    }
  }
  
  const handleClearDatabase = async () => {
    if (!clearTarget) return
    
    setClearing(true)
    setMsg('')
    try {
      const result = await invoke<string>('server_mysql_clear_database', {
        sessionId,
        dbName: clearTarget.name,
      })
      setMsg(result)
      setClearTarget(null)
      setClearConfirmText('')
      await fetchDatabases()
    } catch (e) {
      setMsg(`Clear failed: ${String(e)}`)
    } finally {
      setClearing(false)
    }
  }
  
  const handleChangeAccess = async () => {
    if (!accessTarget) return
    
    // Validate IP if access type is 'ip'
    if (newAccessType === 'ip') {
      const ips = newAllowedIp.split('\n').map(line => line.trim()).filter(line => line.length > 0)
      if (ips.length === 0) {
        setMsg(t('database.enterAllowedIp'))
        return
      }
      // Basic validation: check for empty lines or invalid characters
      const invalidLines = ips.filter(ip => !/^([0-9]{1,3}\.){3}[0-9]{1,3}(\/[0-9]{1,2})?$/.test(ip) && ip !== '%')
      if (invalidLines.length > 0) {
        setMsg(`Invalid IP format: ${invalidLines.join(', ')}`)
        return
      }
    }
    
    setChangingAccess(true)
    setMsg('')
    try {
      const existingCred = dbCredentials[accessTarget.name]
      const result = await invoke<string>('server_mysql_change_db_access', {
        sessionId,
        dbName: accessTarget.name,
        dbUser: accessTarget.user,
        dbPass: existingCred?.password || '',
        accessType: newAccessType,
        allowedIp: newAccessType === 'ip' ? newAllowedIp.trim() : ''
      })
      setMsg(result)
      
      // Save access permission to SQLite
      try {
        await invoke<string>('server_save_db_credentials', {
          sessionId,
          dbName: accessTarget.name,
          dbUser: accessTarget.user,
          password: existingCred?.password || '',
          accessType: newAccessType,
          allowedIp: newAccessType === 'ip' ? newAllowedIp.trim() : ''
        })
      } catch (e) {
        console.error('Failed to save access credentials:', e)
      }
      
      setShowAccessDialog(false)
      setAccessTarget(null)
      setNewAccessType('local')
      setNewAllowedIp('')
      await fetchDatabases()
    } catch (e) {
      setMsg(`${t('common.error')}: ${String(e)}`)
    } finally {
      setChangingAccess(false)
    }
  }
  
  const togglePasswordVisibility = (dbName: string) => {
    const newSet = new Set(visiblePasswords)
    if (newSet.has(dbName)) {
      newSet.delete(dbName)
    } else {
      newSet.add(dbName)
    }
    setVisiblePasswords(newSet)
  }
  
  const openAccessDialog = (db: DbInfo) => {
    setAccessTarget({ name: db.name, user: db.user || db.name })
    // Get current access type from SQLite credentials
    const cred = dbCredentials[db.name]
    setNewAccessType((cred?.access_type as any) || 'local')
    setNewAllowedIp(cred?.allowed_ip || '')
    setShowAccessDialog(true)
  }
  
  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text)
    setMsg(t('database.copiedToClipboard'))
  }
  
  const toggleSelectAll = () => {
    if (selectedDbs.size === paginatedDatabases.length) {
      setSelectedDbs(new Set())
    } else {
      setSelectedDbs(new Set(paginatedDatabases.map(db => db.name)))
    }
  }
  
  const toggleSelectDb = (dbName: string) => {
    const newSet = new Set(selectedDbs)
    if (newSet.has(dbName)) {
      newSet.delete(dbName)
    } else {
      newSet.add(dbName)
    }
    setSelectedDbs(newSet)
  }
  
  const handleBatchOperation = () => {
    if (selectedDbs.size === 0) {
      setMsg(t('database.selectDatabases'))
      return
    }
    setMsg(`Batch operation: ${selectedDbs.size} databases selected`)
  }
  
  const handleChangeRootPassword = async () => {
    if (!newRootPassword.trim()) {
      setMsg(t('database.enterNewPassword'))
      return
    }
    if (newRootPassword.length < 6) {
      setMsg(t('database.enterNewPassword'))
      return
    }
    
    setChangingPw(true)
    try {
      const result = await invoke<string>('server_change_mysql_root_password', { 
        sessionId, 
        newPassword: newRootPassword 
      })
      setMsg(result)
      setShowChangePwDialog(false)
      setNewRootPassword('')
    } catch (e) {
      setMsg(`${t('common.error')}: ` + String(e))
    } finally {
      setChangingPw(false)
    }
  }
  
  const handleDoubleClickRemark = (dbName: string) => {
    setEditingRemark(dbName)
    setRemarkInput(dbRemarks[dbName] || '')
  }
  
  const handleSaveRemark = async (dbName: string) => {
    if (!sessionId) return
    
    const trimmed = remarkInput.trim()
    setDbRemarks(prev => ({
      ...prev,
      [dbName]: trimmed
    }))
    setEditingRemark(null)
    setRemarkInput('')
    
    // Save to SQLite via backend
    try {
      await invoke<string>('server_save_db_remark', {
        sessionId,
        dbName,
        remark: trimmed
      })
      if (trimmed) {
        setMsg(`Updated remark for database "${dbName}"`)
      }
    } catch (e) {
      console.error('Failed to save remark:', e)
      setMsg(t('database.saveRemarkFailed'))
    }
  }
  
 const handleCancelEditRemark = () => {
    setEditingRemark(null)
    setRemarkInput('')
  }
  
  // ===== Backup and Import handlers =====
  
  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
  }
  
  const openBackupDialog = async (dbName: string) => {
    setBackupTarget(dbName)
    setShowBackupDialog(true)
    setLoadingBackups(true)
    setBackups([])
    try {
      const list = await invoke<BackupInfo[]>('server_list_db_backups', { sessionId, dbName })
      setBackups(list)
    } catch (e) {
      console.error('Failed to load backups:', e)
    } finally {
      setLoadingBackups(false)
    }
  }
  
  const handleBackup = async () => {
    if (!backupTarget || !sessionId) return
    const dbPassword = dbCredentials[backupTarget]?.password || ''
    if (!dbPassword) {
      setMsg(t('database.dbNameNotSaved'))
      return
    }
    setBackingUp(true)
    try {
      const result = await invoke<string>('server_backup_database', { 
        sessionId, dbName: backupTarget, dbUser: dbCredentials[backupTarget]?.db_user || backupTarget, dbPassword 
      })
      setMsg(result)
      // Refresh backup list
      const list = await invoke<BackupInfo[]>('server_list_db_backups', { sessionId, dbName: backupTarget })
      setBackups(list)
    } catch (e) {
      setMsg(`${t('common.error')}: ` + String(e))
    } finally {
      setBackingUp(false)
    }
  }
  
  const handleDeleteBackup = async (filename: string) => {
    if (!backupTarget || !sessionId) return
    try {
      await invoke<string>('server_delete_db_backup', { sessionId, backupFilename: filename })
      setMsg(`Backup deleted: ${filename}`)
      // Refresh backup list
      const list = await invoke<BackupInfo[]>('server_list_db_backups', { sessionId, dbName: backupTarget })
      setBackups(list)
    } catch (e) {
      setMsg(`${t('common.error')}: ` + String(e))
    }
  }
  
  const handleDownloadBackup = async (filename: string) => {
    if (!sessionId) return
    try {
      // Call backend to save backup file locally with dialog
      const localPath = await invoke<string>('server_save_db_backup_to_local', { 
        sessionId, 
        backupFilename: filename 
      })
      
      setMsg(`Backup downloaded to: ${localPath}`)
    } catch (e) {
      if (String(e) !== 'Save cancelled') {
        setMsg('Failed to download backup: ' + String(e))
      }
    }
  }
  
  const openImportDialog = async (dbName: string) => {
    setImportTarget(dbName)
    setImportMode('upload')
    setSelectedFile(null)
    setSelectedBackup(null)
    setShowImportDialog(true)
    // Load backups for the backup mode
    setLoadingImportBackups(true)
    try {
      const list = await invoke<BackupInfo[]>('server_list_db_backups', { sessionId, dbName })
      setImportBackups(list)
    } catch (e) {
      console.error('Failed to load backups for import:', e)
    } finally {
      setLoadingImportBackups(false)
    }
  }
  
  const checkUnzipAvailable = async (): Promise<boolean> => {
    if (!sessionId) return false
    try {
      const [, , exitCode] = await invoke<[string, string, number]>('ssh_exec', {
        sessionId,
        command: 'command -v unzip',
      })
      return exitCode === 0
    } catch {
      return false
    }
  }
  
  const handleImport = async () => {
    if (!importTarget || !sessionId) return
    const dbPassword = dbCredentials[importTarget]?.password || ''
    if (!dbPassword) {
      setMsg(t('database.dbNameNotSaved'))
      return
    }
    
    setImporting(true)
    try {
      if (importMode === 'upload' && selectedFile) {
        // Check unzip availability for zip files
        if (selectedFile.name.endsWith('.zip')) {
          const hasUnzip = await checkUnzipAvailable()
          if (!hasUnzip) {
            setMissingToolModal(true)
            setImporting(false)
            return
          }
        }
        // Read file as raw bytes (supports .sql, .tar.gz, .zip)
        // Pass ArrayBuffer directly for efficient binary transfer via Tauri IPC
        const buffer = await selectedFile.arrayBuffer()
        const result = await invoke<string>('server_import_database_from_file_bytes', {
          sessionId,
          dbName: importTarget,
          dbUser: dbCredentials[importTarget]?.db_user || importTarget,
          dbPassword,
          fileName: selectedFile.name,
          fileBytes: buffer
        })
        setMsg(result)
      } else if (importMode === 'backup' && selectedBackup) {
        if (selectedBackup.endsWith('.zip')) {
          const hasUnzip = await checkUnzipAvailable()
          if (!hasUnzip) {
            setMissingToolModal(true)
            setImporting(false)
            return
          }
        }
        const result = await invoke<string>('server_import_database_from_backup', {
          sessionId,
          dbName: importTarget,
          dbUser: dbCredentials[importTarget]?.db_user || importTarget,
          dbPassword,
          backupFilename: selectedBackup
        })
        setMsg(result)
      } else {
        setMsg(t('database.selectFileImport'))
        setImporting(false)
        return
      }
      setShowImportDialog(false)
    } catch (e) {
      setMsg(t('database.importFailed', { error: String(e) }))
    } finally {
      setImporting(false)
    }
  }
  
  const handleImportFromBackup = async (filename: string) => {
    if (!backupTarget || !sessionId) return
    const dbPassword = dbCredentials[backupTarget]?.password || ''
    if (!dbPassword) {
      setMsg(t('database.dbNameNotSaved'))
      return
    }
    // Close backup dialog and import directly
    setShowBackupDialog(false)
    if (filename.endsWith('.zip')) {
      const hasUnzip = await checkUnzipAvailable()
      if (!hasUnzip) {
        setMissingToolModal(true)
        return
      }
    }
    setImporting(true)
    try {
      const result = await invoke<string>('server_import_database_from_backup', {
        sessionId,
        dbName: backupTarget,
        dbUser: dbCredentials[backupTarget]?.db_user || backupTarget,
        dbPassword,
        backupFilename: filename
      })
      setMsg(result)
    } catch (e) {
      setMsg(t('database.importFailed', { error: String(e) }))
    } finally {
      setImporting(false)
    }
  }
  
  return (
    <div className="panel-container">
      <div className="panel-header">
        <h2>{t('database.title')}</h2>
        <div style={{ display: 'flex', gap: '8px' }}>
          <button 
            className="btn-secondary"
            onClick={fetchDatabases}
            disabled={loading}
          >
            {loading ? t('common.loading') : t('common.refresh')}
          </button>
          <button className="btn-secondary" onClick={() => setShowChangePwDialog(true)}>
            {t('database.changeRootPw')}
          </button>
          <button className="btn-primary" onClick={() => setShowCreateDialog(true)}>
            {t('database.addDatabase')}
          </button>
        </div>
      </div>
      
      {msg && (
        <div className={`alert ${msg.includes('failed') || msg.includes('Failed') ? 'alert-error' : 'alert-success'}`} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <span>{msg}</span>
          <button onClick={() => setMsg('')} style={{ background: 'none', border: 'none', color: '#8b949e', fontSize: '18px', cursor: 'pointer', padding: '0 4px', lineHeight: 1 }} title="Close">×</button>
        </div>
      )}
      
      {error && (
        (error.includes('command not found') || error.toLowerCase().includes('mysql')) ? (
          <ServiceUnavailable serviceName="MySQL" onNavigate={onNavigateToSoftware} />
        ) : (
          <div className="alert alert-error">{error}</div>
        )
      )}
      
      {/* Search and filters */}
      <div className="toolbar">
        <input
          type="text"
          placeholder={t('database.searchPlaceholder')}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="search-input"
        />
      </div>
      
      {/* ponytail: search results hint */}
      {searchQuery && (
        <div style={{ color: '#ef4444', marginBottom: '12px', fontSize: '14px' }}>
          {t('database.searchResultsHint')}
        </div>
      )}
      
      {/* Database table */}
      <div className="table-wrapper">
        <table className="data-table">
          <thead>
            <tr>
              <th style={{ width: '40px' }}>
                <input
                  type="checkbox"
                  checked={paginatedDatabases.length > 0 && selectedDbs.size === paginatedDatabases.length}
                  onChange={toggleSelectAll}
                />
              </th>
              <th>{t('database.database')}</th>
              <th>{t('database.user')}</th>
              <th>{t('database.password')}</th>
              <th>{t('database.backup')}</th>
              <th>{t('database.location')}</th>
              <th>{t('database.remark')}</th>
              <th>{t('database.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr>
                <td colSpan={9} style={{ textAlign: 'center', padding: '2rem' }}>
                  {t('common.loading')}
                </td>
              </tr>
            ) : paginatedDatabases.length === 0 ? (
              <tr>
                <td colSpan={9} style={{ textAlign: 'center', padding: '2rem' }}>
                  {filteredDatabases.length === 0 ? t('database.noMatching') : t('database.noDatabases')}
                </td>
              </tr>
            ) : (
              paginatedDatabases.map((db) => (
                <tr key={db.name}>
                  <td>
                    <input
                      type="checkbox"
                      checked={selectedDbs.has(db.name)}
                      onChange={() => toggleSelectDb(db.name)}
                    />
                  </td>
                  <td>{db.name}</td>
                  <td>{db.user || db.name}</td>
                  <td>
                    <span style={{ fontFamily: 'monospace' }}>
                      {visiblePasswords.has(db.name) ? (
                        db.password || t('database.notSaved')
                      ) : (
                        '••••••••'
                      )}
                    </span>
                    <button
                      className="icon-btn"
                      onClick={() => togglePasswordVisibility(db.name)}
                      title={db.password ? (visiblePasswords.has(db.name) ? t('database.hidePassword') : t('database.showPassword')) : t('database.passwordNotSaved')}
                      disabled={!db.password}
                      style={{ opacity: db.password ? 1 : 0.3, cursor: db.password ? 'pointer' : 'not-allowed', fontSize: '14px', lineHeight: 1, padding: '2px 4px' }}
                    >
                      <span style={visiblePasswords.has(db.name) ? {} : { textDecoration: 'line-through', textDecorationColor: '#f85149', textDecorationThickness: '2px' }}>👁️</span>
                    </button>
                    <button
                      className="icon-btn"
                      onClick={() => {
                        if (db.password) {
                          copyToClipboard(db.password)
                        } else {
                          setMsg(t('database.passwordNotSavedMsg'))
                          setTimeout(() => setMsg(''), 2000)
                        }
                      }}
                      title={db.password ? t('database.copyPassword') : t('database.passwordNotSaved')}
                      disabled={!db.password}
                      style={{ opacity: db.password ? 1 : 0.3, cursor: db.password ? 'pointer' : 'not-allowed' }}
                    >
                      📋
                    </button>
                  </td>
                 <td>
                    <span 
                      className="link-text" 
                      onClick={() => openBackupDialog(db.name)}
                      style={{ cursor: 'pointer' }}
                    >
                      {t('database.backup')}
                    </span>
                    {' | '}
                    <span 
                      className="link-text" 
                      onClick={() => openImportDialog(db.name)}
                      style={{ cursor: 'pointer' }}
                    >
                      Import
                    </span>
                  </td>
                  <td>{t('database.location')}</td>
                  <td 
                    onDoubleClick={() => handleDoubleClickRemark(db.name)}
                    style={{ cursor: 'pointer', userSelect: 'none' }}
                    title={t('database.editRemark')}
                  >
                    {editingRemark === db.name ? (
                      <input
                        type="text"
                        value={remarkInput}
                        onChange={(e) => setRemarkInput(e.target.value)}
                        onBlur={() => handleSaveRemark(db.name)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') {
                            handleSaveRemark(db.name)
                          } else if (e.key === 'Escape') {
                            handleCancelEditRemark()
                          }
                        }}
                        onClick={(e) => e.stopPropagation()}
                        className="form-input"
                        autoFocus
                        style={{ width: '100%', padding: '4px 8px', fontSize: '12px' }}
                      />
                    ) : (
                      <span>{dbRemarks[db.name] || '---'}</span>
                    )}
                  </td>
                  <td className="actions">
                    <button 
                      className="action-link"
                      onClick={() => openAccessDialog(db)}
                      title={t('database.accessControl')}
                    >
                      {t('database.accessControl')}
                    </button>
                    <span className="separator">|</span>
                    <button className="action-link" onClick={() => { setChangePwTarget({ name: db.name, user: db.user || db.name }); setNewDbPassword(''); setShowChangePwDbDialog(true); }}>{t('database.changePassword')}</button>
                    <span className="separator">|</span>
                    <button 
                      className="action-link"
                      style={{ color: '#f0883e' }}
                      onClick={() => { setClearTarget(db); setClearConfirmText(''); }}
                    >
                      Clear
                    </button>
                    <span className="separator">|</span>
                    <button 
                      className="action-link danger"
                      onClick={() => setDeleteTarget(db)}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
      
      {/* Bottom toolbar with batch operations and pagination */}
      <div className="bottom-toolbar">
        <div className="batch-ops">
          <select className="select-box">
            <option>{t('database.batchOperations')}</option>
            <option>{t('database.backupAll', { count: selectedDbs.size })}</option>
            <option>{t('database.deleteSelected', { count: selectedDbs.size })}</option>
          </select>
          <button className="btn-secondary" onClick={handleBatchOperation}>
            {t('database.batchOperations')}
          </button>
        </div>
        
        <div className="pagination">
          <button 
            className="page-btn"
            disabled={currentPage === 1}
            onClick={() => setCurrentPage(prev => prev - 1)}
          >
            &lt;
          </button>
          <span className="page-info">{currentPage}</span>
          <button 
            className="page-btn"
            disabled={currentPage >= totalPages}
            onClick={() => setCurrentPage(prev => prev + 1)}
          >
            &gt;
          </button>
          
          <select 
            className="page-size-select"
            value={pageSize}
            onChange={(e) => setPageSize(Number(e.target.value))}
          >
            <option value={10}>10/page</option>
            <option value={20}>20/page</option>
            <option value={50}>50/page</option>
          </select>
          
          <span className="total-info">
            Total: {filteredDatabases.length}
          </span>
          
          <span className="goto-page">
            Go to
            <input 
              type="number" 
              min={1} 
              max={totalPages}
              value={currentPage}
              onChange={(e) => {
                const page = Number(e.target.value)
                if (page >= 1 && page <= totalPages) {
                  setCurrentPage(page)
                }
              }}
              className="page-input"
            />
          </span>
        </div>
      </div>
      
      {/* Create Database Dialog */}
      {showCreateDialog && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
              <h3 style={{ margin: 0 }}>{t('database.createDatabase')}</h3>
              <button 
                onClick={() => setShowCreateDialog(false)}
                style={{ background: 'none', border: 'none', color: '#8b949e', fontSize: '24px', cursor: 'pointer', padding: '0', lineHeight: 1 }}
                title="Close"
              >
                ×
              </button>
            </div>
            
            <div style={{ display: 'flex', gap: '12px' }}>
              <div className="form-group">
                <label><span style={{ color: '#ff4d4f' }}>*</span> {t('database.databaseName')}:</label>
                <input
                  type="text"
                  value={newDbName}
                  onChange={(e) => {
                    const dbName = e.target.value
                    setNewDbName(dbName)
                    // Auto-sync username with database name (always sync)
                    setNewDbUser(dbName)
                  }}
                  placeholder="e.g.: mydb"
                  className="form-input"
                  style={{ width: '160px' }}
                />
              </div>
              
              <div className="form-group">
                <label><span style={{ color: '#ff4d4f' }}>*</span> {t('database.userName')}:</label>
                <input
                  type="text"
                  value={newDbUser}
                  onChange={(e) => setNewDbUser(e.target.value)}
                  placeholder="e.g.: myuser"
                  className="form-input"
                  style={{ width: '160px' }}
                />
              </div>
            </div>
            
            <div style={{ display: 'flex', gap: '12px', alignItems: 'flex-end' }}>
              <div className="form-group" style={{ flex: 3 }}>
                <label><span style={{ color: '#ff4d4f' }}>*</span> {t('database.password')}:</label>
                <div style={{ display: 'flex', gap: '8px' }}>
                  <input
                    type="text"
                    value={newDbPass}
                    onChange={(e) => setNewDbPass(e.target.value)}
                    placeholder="Enter password"
                    className="form-input"
                    style={{ flex: 1 }}
                  />
                  <button 
                    type="button"
                    className="btn-secondary"
                    onClick={() => {
                      const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*'
                      let pass = ''
                      for (let i = 0; i < 16; i++) {
                        pass += chars.charAt(Math.floor(Math.random() * chars.length))
                      }
                      setNewDbPass(pass)
                    }}
                    title={t('database.generatePassword')}
                    style={{ padding: '6px 8px', fontSize: '14px', lineHeight: 1, minWidth: 'auto' }}
                  >
                    🔄
                  </button>
                </div>
              </div>
              
              <div className="form-group" style={{ flex: 1, maxWidth: '140px' }}>
                <label>Charset:</label>
                <select
                  value={dbCharset}
                  onChange={(e) => setDbCharset(e.target.value)}
                  className="form-input"
                >
                  <option value="utf8mb4">utf8mb4</option>
                  <option value="utf8">utf8</option>
                  <option value="gbk">gbk</option>
                  <option value="big5">big5</option>
                  <option value="latin1">latin1</option>
                </select>
              </div>
            </div>
            
            <div className="form-group">
              <label>{t('database.accessControl')}:</label>
              <select
                value={accessType}
                onChange={(e) => setAccessType(e.target.value as any)}
                className="form-input"
              >
                <option value="local">{t('database.localServer')}</option>
                <option value="any">{t('database.allHosts')}</option>
                <option value="ip">{t('database.allowedIps')}</option>
              </select>
            </div>
            
            {accessType === 'ip' && (
              <div className="form-group">
                <label>{t('database.allowedIps')}:</label>
                <textarea
                  value={allowedIp}
                  onChange={(e) => {
                    // Auto-convert spaces/commas to newlines
                    const val = e.target.value.replace(/[,，\s]+/g, '\n')
                    setAllowedIp(val)
                  }}
                  placeholder={`One IP address or range per line, e.g.:
192.168.1.100
192.168.1.%
10.0.0.0/8`}
                  className="form-input"
                  style={{ minHeight: '100px', resize: 'vertical' as const, fontFamily: 'monospace', fontSize: '13px' }}
                />
                <small style={{ color: '#8b949e', fontSize: '12px' }}>
                  One IP (192.168.1.100) or IP range (192.168.1.% or 10.0.0.0/8) per line
                </small>
              </div>
            )}
            
            <div style={{ marginBottom: '16px' }} title="Note: MySQL passwords are encrypted and cannot be read from config. We save them to the sqlite database in the software directory.">
              <label style={{ display: 'flex', alignItems: 'center', gap: '8px', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={savePasswordLocally}
                  onChange={(e) => setSavePasswordLocally(e.target.checked)}
                  style={{ width: 'auto', margin: 0 }}
                />
                <span>{t('database.savePasswordLocally')}</span>
              </label>
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowCreateDialog(false)}
                disabled={creating}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-primary"
                onClick={handleCreateDatabase}
                disabled={creating}
              >
                {creating ? t('common.loading') : t('common.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Delete Confirmation Dialog */}
      {deleteTarget && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => { setDeleteTarget(null); setDeleteConfirmName(''); }}
              title="Close"
            >×</button>
            <h3>{t('database.deleteDatabase')}</h3>
            <p style={{ color: '#f85149', fontSize: '13px', margin: '8px 0' }}>{t('database.deleteConfirm')} "<strong>{deleteTarget.name}</strong>". {t('common.warning')}!</p>
            
            <div className="form-group">
              <label style={{ fontSize: '13px' }}>{t('database.databaseName')}: <code style={{ background: '#21262d', padding: '2px 6px', borderRadius: '4px', color: '#f85149' }}>{deleteTarget.name}</code></label>
              <input
                type="text"
                value={deleteConfirmName}
                onChange={(e) => setDeleteConfirmName(e.target.value)}
                placeholder={deleteTarget.name}
                className="form-input"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && deleteConfirmName.trim().toLowerCase() === deleteTarget.name.toLowerCase()) {
                    handleDeleteDatabase()
                  } else if (e.key === 'Escape') {
                    setDeleteTarget(null)
                    setDeleteConfirmName('')
                  }
                }}
              />
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => { setDeleteTarget(null); setDeleteConfirmName(''); }}
                disabled={deleting}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-danger"
                onClick={handleDeleteDatabase}
                disabled={deleting || deleteConfirmName.trim().toLowerCase() !== deleteTarget.name.toLowerCase()}
              >
                {deleting ? t('common.deleting') : t('common.delete')}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Clear Database Dialog */}
      {clearTarget && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => { setClearTarget(null); setClearConfirmText(''); }}
              title="Close"
            >×</button>
            <h3>Clear Database</h3>
            <p style={{ color: '#f85149', fontSize: '13px', margin: '8px 0' }}>This will truncate ALL tables in database "<strong>{clearTarget.name}</strong>". The database and user will be preserved. Type <code style={{ background: '#21262d', padding: '2px 6px', borderRadius: '4px' }}>clear</code> to confirm.</p>
            
            <div className="form-group">
              <label style={{ fontSize: '13px' }}>Type "clear" to confirm:</label>
              <input
                type="text"
                value={clearConfirmText}
                onChange={(e) => setClearConfirmText(e.target.value)}
                placeholder="clear"
                className="form-input"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && clearConfirmText.trim().toLowerCase() === 'clear') {
                    handleClearDatabase()
                  } else if (e.key === 'Escape') {
                    setClearTarget(null)
                    setClearConfirmText('')
                  }
                }}
              />
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => { setClearTarget(null); setClearConfirmText(''); }}
                disabled={clearing}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-danger"
                onClick={handleClearDatabase}
                disabled={clearing || clearConfirmText.trim().toLowerCase() !== 'clear'}
              >
                {clearing ? t('common.loading') : 'Clear'}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Change Root Password Dialog */}
      {showChangePwDialog && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setShowChangePwDialog(false)}
              title="Close"
            >×</button>
            <h3>{t('database.changeRootPassword')}</h3>
                  
            <div className="form-group">
              <label>{t('database.newPassword')}:</label>
              <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                <input
                  type={showRootPassword ? 'text' : 'password'}
                  value={newRootPassword}
                  onChange={(e) => setNewRootPassword(e.target.value)}
                  placeholder="Enter new password (min 6 chars)"
                  className="form-input"
                  style={{ flex: 1 }}
                />
                <button
                  type="button"
                  onClick={() => setShowRootPassword(!showRootPassword)}
                  title={showRootPassword ? t('database.hidePassword') : t('database.showPassword')}
                  style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: '18px', padding: '4px 6px', lineHeight: 1 }}
                >
                  {showRootPassword ? '🙈' : '👁'}
                </button>
              </div>
            </div>
                  
            <div style={{ marginBottom: '12px', fontSize: '12px', color: '#888' }}>
              ⚠️ Note: Please update all application configurations using this password after changing it.
            </div>
                  
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowChangePwDialog(false)}
                disabled={changingPw}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-primary"
                onClick={handleChangeRootPassword}
                disabled={changingPw}
              >
                {changingPw ? t('common.loading') : t('common.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}
            
      {/* Change Access Permission Dialog */}
      {showAccessDialog && accessTarget && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
              <h3 style={{ margin: 0 }}>{t('database.accessControl')} - {accessTarget.name}</h3>
              <button 
                onClick={() => setShowAccessDialog(false)}
                style={{ background: 'none', border: 'none', color: '#8b949e', fontSize: '24px', cursor: 'pointer', padding: '0', lineHeight: 1 }}
                title="Close"
              >
                ×
              </button>
            </div>
                  
            <div className="form-group">
              <label>{t('database.accessControl')}:</label>
              <select
                value={newAccessType}
                onChange={(e) => setNewAccessType(e.target.value as any)}
                className="form-input"
              >
                <option value="local">{t('database.localServer')}</option>
                <option value="any">{t('database.allHosts')}</option>
                <option value="ip">{t('database.allowedIps')}</option>
              </select>
            </div>
                  
            {newAccessType === 'ip' && (
              <div className="form-group">
                <label>{t('database.allowedIps')}:</label>
                <textarea
                  value={newAllowedIp}
                  onChange={(e) => {
                    // Auto-convert spaces/commas to newlines
                    const val = e.target.value.replace(/[,，\s]+/g, '\n')
                    setNewAllowedIp(val)
                  }}
                  placeholder={`One IP address or range per line, e.g.:
192.168.1.100
192.168.1.%
10.0.0.0/8`}
                  className="form-input"
                  style={{ minHeight: '100px', resize: 'vertical' as const, fontFamily: 'monospace', fontSize: '13px' }}
                />
                <small style={{ color: '#8b949e', fontSize: '12px' }}>
                  One IP (192.168.1.100) or IP range (192.168.1.% or 10.0.0.0/8) per line
                </small>
              </div>
            )}
                  
            <div style={{ marginBottom: '12px', fontSize: '12px', color: '#888' }}>
               Tip: Changing access permissions requires re-authorizing the database user.
            </div>
                  
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowAccessDialog(false)}
                disabled={changingAccess}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-primary"
                onClick={handleChangeAccess}
                disabled={changingAccess}
              >
                {changingAccess ? t('common.loading') : t('common.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Change DB User Password Dialog */}
      {showChangePwDbDialog && changePwTarget && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setShowChangePwDbDialog(false)}
              title="Close"
            >×</button>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
              <h3 style={{ margin: 0 }}>{t('database.changePassword')} - {changePwTarget.name}</h3>
            </div>
            
            <div className="form-group">
              <label>{t('database.newPassword')}:</label>
              <div style={{ display: 'flex', gap: '8px' }}>
                <input
                  type="text"
                  value={newDbPassword}
                  onChange={(e) => setNewDbPassword(e.target.value)}
                  placeholder="Enter new password (min 6 chars)"
                  className="form-input"
                  style={{ flex: 1 }}
                  autoFocus
                />
                <button 
                  type="button"
                  className="btn-secondary"
                  onClick={() => {
                    const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*'
                    let pass = ''
                    for (let i = 0; i < 16; i++) {
                      pass += chars.charAt(Math.floor(Math.random() * chars.length))
                    }
                    setNewDbPassword(pass)
                  }}
                  title={t('database.generatePassword')}
                  style={{ padding: '6px 8px', fontSize: '14px', lineHeight: 1, minWidth: 'auto' }}
                >
                  🔄
                </button>
              </div>
            </div>
            
            <div style={{ marginBottom: '16px' }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: '8px', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={updateLocalPassword}
                  onChange={(e) => setUpdateLocalPassword(e.target.checked)}
                  style={{ width: 'auto', margin: 0 }}
                />
                <span>{t('database.savePasswordLocally')}</span>
              </label>
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowChangePwDbDialog(false)}
                disabled={changingDbPw}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-primary"
                onClick={async () => {
                  if (!newDbPassword.trim()) { setMsg(t('database.enterNewPassword')); return }
                  if (newDbPassword.length < 6) { setMsg(t('database.enterNewPassword')); return }
                  setChangingDbPw(true)
                  try {
                    const cred = dbCredentials[changePwTarget.name]
                    const savedAccessType = cred?.access_type || 'local'
                    const savedAllowedIp = cred?.allowed_ip || ''
                    const result = await invoke<string>('server_change_db_user_password', {
                      sessionId,
                      dbUser: changePwTarget.user,
                      newPassword: newDbPassword,
                      accessType: savedAccessType,
                      allowedIp: savedAllowedIp
                    })
                    setMsg(result)
                    // Update password in SQLite
                    try {
                      await invoke<string>('server_update_db_credential_password', {
                        sessionId,
                        dbName: changePwTarget.name,
                        password: updateLocalPassword ? newDbPassword : ''
                      })
                    } catch (e) {
                      console.error('Failed to update credential password:', e)
                    }
                    await fetchDatabases()
                    setShowChangePwDbDialog(false)
                    setNewDbPassword('')
                  } catch (e) {
                    setMsg(`${t('common.error')}: ` + String(e))
                  } finally {
                    setChangingDbPw(false)
                  }
                }}
                disabled={changingDbPw}
              >
                {changingDbPw ? t('common.loading') : t('common.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Backup Dialog */}
      {showBackupDialog && backupTarget && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()} style={{ maxWidth: '700px' }}>
            <button 
              className="modal-close-btn"
              onClick={() => setShowBackupDialog(false)}
              title="Close"
            >×</button>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
              <h3 style={{ margin: 0 }}>{t('database.backupDatabase')} - {backupTarget}</h3>
            </div>
            
            <div style={{ marginBottom: '16px', fontSize: '13px', color: '#8b949e' }}>
              Backup files are saved in /tmp/db_backups/ on the server (.tar.gz format)
            </div>
            
            {/* Backup list */}
            <div style={{ maxHeight: '300px', overflowY: 'auto', marginBottom: '16px', border: '1px solid #30363d', borderRadius: '6px' }}>
              {loadingBackups ? (
                <div style={{ textAlign: 'center', padding: '20px', color: '#8b949e' }}>{t('common.loading')}</div>
              ) : backups.length === 0 ? (
                <div style={{ textAlign: 'center', padding: '20px', color: '#8b949e' }}>{t('database.noBackups')}</div>
              ) : (
                <table className="data-table" style={{ margin: 0 }}>
                  <thead>
                    <tr>
                      <th style={{ fontSize: '12px' }}>{t('common.name')}</th>
                      <th style={{ fontSize: '12px', width: '80px' }}>{t('common.size')}</th>
                      <th style={{ fontSize: '12px', width: '160px' }}>{t('common.status')}</th>
                      <th style={{ fontSize: '12px', width: '180px' }}>{t('common.actions')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {backups.map((backup) => (
                      <tr key={backup.filename}>
                        <td style={{ fontSize: '12px', fontFamily: 'monospace' }}>{backup.filename}</td>
                        <td style={{ fontSize: '12px' }}>{formatBytes(backup.size_bytes)}</td>
                        <td style={{ fontSize: '12px' }}>{backup.created_at}</td>
                        <td>
                          <button 
                            className="action-link"
                            onClick={() => handleDownloadBackup(backup.filename)}
                            style={{ fontSize: '12px' }}
                          >
                            {t('common.download')}
                          </button>
                          <span className="separator">|</span>
                          <button 
                            className="action-link"
                            onClick={() => handleImportFromBackup(backup.filename)}
                            disabled={importing}
                            style={{ fontSize: '12px' }}
                          >
                            Import
                          </button>
                          <span className="separator">|</span>
                          <button 
                            className="action-link danger"
                            onClick={() => handleDeleteBackup(backup.filename)}
                            style={{ fontSize: '12px' }}
                          >
                            {t('common.delete')}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowBackupDialog(false)}
                disabled={backingUp}
              >
                {t('common.close')}
              </button>
              <button 
                className="btn-primary"
                onClick={handleBackup}
                disabled={backingUp}
              >
                {backingUp ? t('common.loading') : t('database.backupDatabase')}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Import Dialog */}
      {showImportDialog && importTarget && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()} style={{ maxWidth: '500px' }}>
            <button 
              className="modal-close-btn"
              onClick={() => setShowImportDialog(false)}
              title="Close"
            >×</button>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
              <h3 style={{ margin: 0 }}>{t('database.importDatabase')} - {importTarget}</h3>
            </div>
            
            <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
              <button 
                className={importMode === 'upload' ? 'btn-primary' : 'btn-secondary'}
                onClick={() => setImportMode('upload')}
                style={{ flex: 1 }}
              >
                {t('database.importFile')}
              </button>
              <button 
                className={importMode === 'backup' ? 'btn-primary' : 'btn-secondary'}
                onClick={() => setImportMode('backup')}
                style={{ flex: 1 }}
              >
                {t('database.selectBackup')}
              </button>
            </div>
            
            {importMode === 'upload' ? (
              <div className="form-group">
                <label>{t('database.importFile')}:</label>
                <input
                  type="file"
                  accept="*"
                  onChange={(e) => setSelectedFile(e.target.files?.[0] || null)}
                  className="form-input"
                  style={{ padding: '8px' }}
                />
                {selectedFile && (
                  <div style={{ marginTop: '8px', fontSize: '12px', color: '#8b949e' }}>
                    Selected: {selectedFile.name} ({formatBytes(selectedFile.size)})
                  </div>
                )}
              </div>
            ) : (
              <div className="form-group">
                <label>{t('database.selectBackup')}:</label>
                {loadingImportBackups ? (
                  <div style={{ padding: '12px', textAlign: 'center', color: '#8b949e' }}>{t('common.loading')}</div>
                ) : importBackups.length === 0 ? (
                  <div style={{ padding: '12px', textAlign: 'center', color: '#8b949e', border: '1px solid #30363d', borderRadius: '6px' }}>
                    {t('database.noBackups')}
                  </div>
                ) : (
                  <select
                    value={selectedBackup || ''}
                    onChange={(e) => setSelectedBackup(e.target.value)}
                    className="form-input"
                  >
                    <option value="">-- {t('database.selectBackup')} --</option>
                    {importBackups.map((backup) => (
                      <option key={backup.filename} value={backup.filename}>
                        {backup.filename} ({formatBytes(backup.size_bytes)}) - {backup.created_at}
                      </option>
                    ))}
                  </select>
                )}
              </div>
            )}
            
            <div style={{ marginBottom: '16px', padding: '10px', background: '#f8514922', borderRadius: '6px', fontSize: '12px', color: '#f85149' }}>
              ⚠️ {t('common.warning')}: Importing will overwrite existing data.
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowImportDialog(false)}
                disabled={importing}
              >
                {t('common.cancel')}
              </button>
              <button 
                className="btn-primary"
                onClick={handleImport}
                disabled={importing || (importMode === 'upload' ? !selectedFile : !selectedBackup)}
              >
                {importing ? t('common.loading') : t('database.importDatabase')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Missing unzip tool modal */}
      {missingToolModal && (
        <div className="modal-overlay" onClick={() => setMissingToolModal(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()} style={{ maxWidth: '400px', textAlign: 'center' }}>
            <button className="modal-close-btn" onClick={() => setMissingToolModal(false)} title="Close">×</button>
            <div style={{ marginBottom: '16px', fontSize: '16px', fontWeight: 600 }}>{t('files.missingToolTitle')}</div>
            <div style={{ marginBottom: '20px', fontSize: '13px', color: '#8b949e' }}>
              {t('files.missingUnzipTool')}
            </div>
            <div style={{ display: 'flex', gap: '8px', justifyContent: 'center' }}>
              <button className="btn-secondary" onClick={() => setMissingToolModal(false)}>
                {t('common.cancel')}
              </button>
              <button className="btn-primary" onClick={() => {
                setMissingToolModal(false)
                onNavigateToSoftware?.()
              }}>
                {t('database.goToSoftware')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
