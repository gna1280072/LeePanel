import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface RedisKeyInfo {
  key: string
  value_preview: string
  data_type: string
  length: number
  ttl: number // -1 means no expiry
}

interface RedisDbSize {
  db_index: number
  key_count: number
}

interface BackupInfo {
  filename: string
  size_bytes: number
  created_at: string
}

interface RedisPanelProps {
  sessionId: string | null
  onNavigateToSoftware?: () => void
}

export default function RedisPanel({ sessionId, onNavigateToSoftware }: RedisPanelProps) {
  const [redisStatus, setRedisStatus] = useState<'checking' | 'running' | 'stopped' | 'not_installed'>('checking')
  const [redisVersion, setRedisVersion] = useState<string>('')
  const [dbSizes, setDbSizes] = useState<RedisDbSize[]>([])
  const [currentDb, setCurrentDb] = useState<number>(0)
  const [keys, setKeys] = useState<RedisKeyInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [msg, setMsg] = useState('')
  
  // Search and pagination
  const [searchQuery, setSearchQuery] = useState('')
  const [searchType, setSearchType] = useState<'key' | 'value'>('key')
  const [pageSize, setPageSize] = useState(50)
  const [totalKeys, setTotalKeys] = useState(0)
  const [cursor, setCursor] = useState<number>(0)
  
  // Dialogs
  const [showAddDialog, setShowAddDialog] = useState(false)
  const [newKeyName, setNewKeyName] = useState('')
  const [newValue, setNewValue] = useState('')
  const [newTTL, setNewTTL] = useState<string>('')
  const [adding, setAdding] = useState(false)
  
  const [deleteTarget, setDeleteTarget] = useState<RedisKeyInfo | null>(null)
  const [deleting, setDeleting] = useState(false)
  
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set())
  
  const [showBackupDialog, setShowBackupDialog] = useState(false)
  const [backups, setBackups] = useState<BackupInfo[]>([])
  const [loadingBackups, setLoadingBackups] = useState(false)
  
  // Flush DB confirmation modal
  const [showFlushConfirm, setShowFlushConfirm] = useState(false)
  const [flushInput, setFlushInput] = useState('')
  
  const [creatingBackup, setCreatingBackup] = useState(false)
  
  // Check Redis status on mount
  useEffect(() => {
    checkRedis()
  }, [])
  
  const checkRedis = async () => {
    if (!sessionId) return
    
    try {
      // Parallelize status check and version fetch
      const [isRunning, version] = await Promise.all([
        invoke<boolean>('server_redis_check_status', { sessionId }),
        invoke<string>('server_redis_get_version', { sessionId }).catch(() => '')
      ])
      
      if (isRunning) {
        setRedisStatus('running')
        setRedisVersion(version || 'Unknown')
        await loadDbSizes()
      } else {
        setRedisStatus('stopped')
      }
    } catch (e) {
      const errorMsg = String(e)
      if (errorMsg.includes('not_installed')) {
        setRedisStatus('not_installed')
      } else {
        setRedisStatus('stopped')
      }
    }
  }
  
  const loadDbSizes = async () => {
    if (!sessionId) return
    
    try {
      setLoading(true)
      setError('')
      
      // Load DB sizes and keys in parallel
      const [sizes, keyResult] = await Promise.all([
        invoke<RedisDbSize[]>('server_redis_dbsize_all', { sessionId }),
        (async () => {
          const pattern = searchQuery ? `*${searchQuery}*` : '*'
          const result = await invoke<[RedisKeyInfo[], number]>('server_redis_scan_keys', {
            sessionId,
            dbIndex: currentDb,
            pattern,
            searchType,
            cursor: 0,
            count: pageSize
          })
          return result
        })()
      ])
      
      setDbSizes(sizes)
      
      const [keyList, nextCursor] = keyResult
      setKeys(keyList)
      setCursor(nextCursor)
      setTotalKeys(sizes.find(d => d.db_index === currentDb)?.key_count || 0)
      
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }
  
  const loadKeys = async (resetCursor = true) => {
    if (!sessionId) return
    
    setLoading(true)
    try {
      const scanCursor = resetCursor ? 0 : cursor
      // Auto-wrap search query with wildcards for Redis SCAN pattern matching
      const pattern = searchQuery ? `*${searchQuery}*` : '*'
      
      const result = await invoke<[RedisKeyInfo[], number]>('server_redis_scan_keys', {
        sessionId,
        dbIndex: currentDb,
        pattern,
        searchType,
        cursor: scanCursor,
        count: pageSize
      })
      
      const [keyList, nextCursor] = result
      
      if (resetCursor) {
        setKeys(keyList)
        setCursor(nextCursor)
        // Use cached total from dbSizes
        setTotalKeys(dbSizes.find(d => d.db_index === currentDb)?.key_count || 0)
      } else {
        setKeys(prev => [...prev, ...keyList])
        setCursor(nextCursor)
      }
      
      setError('')
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }
  
  // Reload when DB changes
  useEffect(() => {
    if (redisStatus === 'running' && dbSizes.length > 0) {
      loadKeys()
    }
  }, [currentDb])
  
  const handleSearch = () => {
    loadKeys(true)
  }
  
  const handleLoadMore = () => {
    if (cursor !== 0) {
      loadKeys(false)
    }
  }
  
  const handleAddKey = async () => {
    if (!newKeyName.trim() || !newValue.trim()) {
      setMsg('Please fill in key name and value')
      return
    }
    
    setAdding(true)
    try {
      const ttl = newTTL.trim() ? parseInt(newTTL) : undefined
      const result = await invoke<string>('server_redis_set_key', {
        sessionId,
        dbIndex: currentDb,
        key: newKeyName,
        value: newValue,
        ttl
      })
      
      setMsg(result)
      setShowAddDialog(false)
      setNewKeyName('')
      setNewValue('')
      setNewTTL('')
      
      // Refresh list
      await loadKeys()
    } catch (e) {
      setMsg('Add failed: ' + String(e))
    } finally {
      setAdding(false)
    }
  }
  
  const handleDeleteKey = async () => {
    if (!deleteTarget) return
    
    setDeleting(true)
    try {
      const deleted = await invoke<number>('server_redis_del_key', {
        sessionId,
        dbIndex: currentDb,
        keys: [deleteTarget.key]
      })
      
      setMsg(`Deleted ${deleted} keys`)
      setDeleteTarget(null)
      
      // Refresh list
      await loadKeys()
    } catch (e) {
      setMsg('Delete failed: ' + String(e))
    } finally {
      setDeleting(false)
    }
  }
  
  const handleBatchDelete = async () => {
    if (selectedKeys.size === 0) {
      setMsg('Please select keys to delete first')
      return
    }
    
    setDeleting(true)
    try {
      const deleted = await invoke<number>('server_redis_del_key', {
        sessionId,
        dbIndex: currentDb,
        keys: Array.from(selectedKeys)
      })
      
      setMsg(`Deleted ${deleted} keys`)
      setSelectedKeys(new Set())
      
      // Refresh list
      await loadKeys()
    } catch (e) {
      setMsg('Batch delete failed: ' + String(e))
    } finally {
      setDeleting(false)
    }
  }
  
  const handleFlushDb = async () => {
    setShowFlushConfirm(true)
    setFlushInput('')
  }
  
  const confirmFlushDb = async () => {
    if (flushInput.toLowerCase() !== 'redis') {
      setMsg('Flush operation cancelled')
      setShowFlushConfirm(false)
      return
    }
    
    try {
      const result = await invoke<string>('server_redis_flushdb', {
        sessionId,
        dbIndex: currentDb
      })
      
      setMsg(result)
      setShowFlushConfirm(false)
      setFlushInput('')
      await loadKeys()
    } catch (e) {
      setMsg('Flush failed: ' + String(e))
      setShowFlushConfirm(false)
    }
  }
  
  const handleCreateBackup = async () => {
    setCreatingBackup(true)
    try {
      const backupPath = await invoke<string>('server_redis_save_backup', { sessionId })
      setMsg(`Backup created: ${backupPath}`)
      await loadBackups()
    } catch (e) {
      setMsg('Backup failed: ' + String(e))
    } finally {
      setCreatingBackup(false)
    }
  }
  
  const loadBackups = async () => {
    if (!sessionId) return
    
    setLoadingBackups(true)
    try {
      const backupList = await invoke<BackupInfo[]>('server_redis_list_backups', { sessionId })
      setBackups(backupList)
    } catch (e) {
      setMsg('Failed to load backup list: ' + String(e))
    } finally {
      setLoadingBackups(false)
    }
  }
  
  const toggleSelectAll = () => {
    if (selectedKeys.size === keys.length) {
      setSelectedKeys(new Set())
    } else {
      setSelectedKeys(new Set(keys.map(k => k.key)))
    }
  }
  
  const toggleSelectKey = (keyName: string) => {
    const newSet = new Set(selectedKeys)
    if (newSet.has(keyName)) {
      newSet.delete(keyName)
    } else {
      newSet.add(keyName)
    }
    setSelectedKeys(newSet)
  }
  
  const formatTTL = (ttl: number): string => {
    if (ttl === -1) return 'Permanent'
    if (ttl < 60) return `${ttl}s`
    if (ttl < 3600) return `${Math.floor(ttl / 60)}m`
    if (ttl < 86400) return `${Math.floor(ttl / 3600)}h`
    return `${Math.floor(ttl / 86400)}d`
  }
  
  const truncateValue = (value: string, maxLength = 100): string => {
    if (value.length <= maxLength) return value
    return value.substring(0, maxLength) + '...'
  }
  
  const getTypeColor = (type: string): string => {
    switch (type) {
      case 'string': return '#4CAF50'
      case 'list': return '#2196F3'
      case 'set': return '#FF9800'
      case 'hash': return '#9C27B0'
      case 'zset': return '#E91E63'
      default: return '#666'
    }
  }
  
  // Render Redis not installed message
  if (redisStatus === 'not_installed') {
    return (
      <div className="panel-container">
        <div className="panel-header">
          <h2>Redis Management</h2>
        </div>
        
        <div className="alert alert-error">
          <div style={{ marginBottom: '12px', fontSize: '14px' }}>
            Redis is not installed. Please go to Software to install it.
          </div>
          {onNavigateToSoftware && (
            <button 
              className="btn-primary"
              onClick={onNavigateToSoftware}
            >
              Go to Software
            </button>
          )}
        </div>
      </div>
    )
  }
  
  // Render Redis stopped message
  if (redisStatus === 'stopped') {
    const handleStartRedis = async () => {
      try {
        setMsg('Starting Redis service...')
        await invoke<string>('server_service_action', { 
          sessionId, 
          service: 'redis', 
          action: 'start' 
        })
        setMsg('Redis service started. Please refresh the page.')
        // Auto-check after 2 seconds
        setTimeout(() => {
          checkRedis()
          setMsg('')
        }, 2000)
      } catch (e) {
        setMsg('Start failed: ' + String(e))
      }
    }
    
    return (
      <div className="panel-container">
        <div className="panel-header">
          <h2>Redis Management</h2>
        </div>
        
        <div className="alert alert-error">
          <div style={{ marginBottom: '12px', fontSize: '14px' }}>
            Redis service is not running. Please start Redis.
          </div>
          <button 
            className="btn-primary"
            onClick={handleStartRedis}
          >
            Start Redis
          </button>
        </div>
      </div>
    )
  }
  
  return (
    <div className="panel-container">
      {/* Header */}
      <div className="panel-header">
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <h2>Redis Management</h2>
          <span style={{ fontSize: '12px', color: '#4CAF50', fontWeight: 'bold' }}>
            Redis {redisVersion} ▶
          </span>
        </div>
        <div style={{ display: 'flex', gap: '8px' }}>
          <button className="btn-primary" onClick={() => setShowAddDialog(true)}>
            Add Key
          </button>
          <button className="btn-secondary" onClick={() => { setShowBackupDialog(true); loadBackups(); }}>
            Backup List
          </button>
          <button className="btn-secondary" onClick={handleFlushDb}>
            Flush Database
          </button>
        </div>
      </div>
      
      {/* Messages */}
      {msg && (
        <div className={`alert ${msg.includes('failed') || msg.includes('Failed') ? 'alert-error' : 'alert-success'}`}>
          {msg}
        </div>
      )}
      
      {error && (
        <div className="alert alert-error">
          {error}
        </div>
      )}
      
      {/* Database Tabs */}
      <div style={{ display: 'flex', overflowX: 'auto', gap: '4px', marginBottom: '16px', paddingBottom: '8px' }}>
        {dbSizes.map((db) => (
          <button
            key={db.db_index}
            className={`tab-btn ${currentDb === db.db_index ? 'active' : ''}`}
            onClick={() => setCurrentDb(db.db_index)}
            style={{
              padding: '8px 16px',
              border: 'none',
              borderRadius: '4px',
              backgroundColor: currentDb === db.db_index ? '#4CAF50' : '#2a2a2a',
              color: currentDb === db.db_index ? '#fff' : '#ccc',
              cursor: 'pointer',
              whiteSpace: 'nowrap'
            }}
          >
            DB{db.db_index} [{db.key_count}]
          </button>
        ))}
      </div>
      
      {/* Search Bar */}
      <div className="toolbar">
        <select 
          className="search-type-select"
          value={searchType}
          onChange={(e) => setSearchType(e.target.value as 'key' | 'value')}
          style={{ marginRight: '8px', padding: '6px 12px', borderRadius: '4px', border: '1px solid #444', backgroundColor: '#2a2a2a', color: '#fff' }}
        >
          <option value="key">Key</option>
          <option value="value">Value</option>
        </select>
        <input
          type="text"
          className="search-input"
          placeholder={searchType === 'key' ? 'Enter key name' : 'Enter key value'}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyPress={(e) => e.key === 'Enter' && handleSearch()}
        />
        <button className="btn-secondary" onClick={handleSearch}>
          🔍
        </button>
      </div>
      
      {/* Keys Table */}
      <div className="table-wrapper">
        <table className="data-table">
          <thead>
            <tr>
              <th style={{ width: '40px' }}>
                <input
                  type="checkbox"
                  checked={keys.length > 0 && selectedKeys.size === keys.length}
                  onChange={toggleSelectAll}
                />
              </th>
              <th>Key</th>
              <th>Value</th>
              <th>Type</th>
              <th>Length</th>
              <th>TTL</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {loading && keys.length === 0 ? (
              <tr>
                <td colSpan={7} style={{ textAlign: 'center', padding: '2rem' }}>
                  Loading...
                </td>
              </tr>
            ) : keys.length === 0 ? (
              <tr>
                <td colSpan={7} style={{ textAlign: 'center', padding: '2rem' }}>
                  Database is empty
                </td>
              </tr>
            ) : (
              keys.map((keyInfo) => (
                <tr key={keyInfo.key}>
                  <td>
                    <input
                      type="checkbox"
                      checked={selectedKeys.has(keyInfo.key)}
                      onChange={() => toggleSelectKey(keyInfo.key)}
                    />
                  </td>
                  <td style={{ fontFamily: 'monospace', maxWidth: '200px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {keyInfo.key}
                  </td>
                  <td style={{ fontFamily: 'monospace', maxWidth: '300px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {truncateValue(keyInfo.value_preview)}
                  </td>
                  <td>
                    <span style={{ 
                      display: 'inline-block',
                      padding: '2px 8px',
                      borderRadius: '3px',
                      backgroundColor: getTypeColor(keyInfo.data_type),
                      color: '#fff',
                      fontSize: '12px'
                    }}>
                      {keyInfo.data_type}
                    </span>
                  </td>
                  <td>{keyInfo.length}</td>
                  <td>{formatTTL(keyInfo.ttl)}</td>
                  <td>
                    <button 
                      className="action-link danger"
                      onClick={() => setDeleteTarget(keyInfo)}
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
      
      {/* Bottom Toolbar */}
      <div className="bottom-toolbar">
        <div className="batch-ops">
          <select 
            className="select-box"
            disabled={selectedKeys.size === 0}
            onChange={(e) => {
              if (e.target.value === 'delete') {
                handleBatchDelete()
              }
            }}
          >
            <option value="">Select batch operation</option>
            <option value="delete">Batch Delete</option>
          </select>
        </div>
        
        <div className="pagination">
          <button 
            className="page-btn"
            onClick={() => loadKeys(true)}
            disabled={loading}
          >
            Refresh
          </button>
          
          {cursor !== 0 && (
            <button 
              className="page-btn"
              onClick={handleLoadMore}
              disabled={loading}
            >
              Load More
            </button>
          )}
          
          <span className="page-info">
            Total: {keys.length} (Overall: {totalKeys})
          </span>
          
          <select 
            className="page-size-select"
            value={pageSize}
            onChange={(e) => {
              setPageSize(Number(e.target.value))
              loadKeys(true)
            }}
          >
            <option value={50}>50/page</option>
            <option value={100}>100/page</option>
            <option value={200}>200/page</option>
          </select>
        </div>
      </div>
      
      {/* Add Key Dialog */}
      {showAddDialog && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setShowAddDialog(false)}
              title="Close"
            >×</button>
            <h3>Add Key</h3>
            
            <div className="form-group">
              <label>Key:</label>
              <input
                type="text"
                value={newKeyName}
                onChange={(e) => setNewKeyName(e.target.value)}
                placeholder="e.g.: mykey"
                className="form-input"
              />
            </div>
            
            <div className="form-group">
              <label>Value:</label>
              <textarea
                value={newValue}
                onChange={(e) => setNewValue(e.target.value)}
                placeholder="Enter value"
                className="form-input"
                rows={4}
                style={{ resize: 'vertical' }}
              />
            </div>
            
            <div className="form-group">
              <label>TTL (seconds, optional):</label>
              <input
                type="number"
                value={newTTL}
                onChange={(e) => setNewTTL(e.target.value)}
                placeholder="Leave empty for permanent"
                className="form-input"
              />
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setShowAddDialog(false)}
                disabled={adding}
              >
                Cancel
              </button>
              <button 
                className="btn-primary"
                onClick={handleAddKey}
                disabled={adding}
              >
                {adding ? 'Adding...' : 'Confirm'}
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
              onClick={() => setDeleteTarget(null)}
              title="Close"
            >×</button>
            <h3>Confirm Delete</h3>
            <p>Are you sure you want to delete key "{deleteTarget.key}"?</p>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => setDeleteTarget(null)}
                disabled={deleting}
              >
                Cancel
              </button>
              <button 
                className="btn-danger"
                onClick={handleDeleteKey}
                disabled={deleting}
              >
                {deleting ? 'Deleting...' : 'Confirm Delete'}
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Backup List Dialog */}
      {showBackupDialog && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()} style={{ maxWidth: '600px' }}>
            <button 
              className="modal-close-btn"
              onClick={() => setShowBackupDialog(false)}
              title="Close"
            >×</button>
            <h3>Backup List</h3>
            
            <div style={{ marginBottom: '16px' }}>
              <button 
                className="btn-primary"
                onClick={handleCreateBackup}
                disabled={creatingBackup}
              >
                {creatingBackup ? 'Creating...' : 'Create Backup'}
              </button>
            </div>
            
            {loadingBackups ? (
              <p>Loading...</p>
            ) : backups.length === 0 ? (
              <p>No backups</p>
            ) : (
              <div className="table-wrapper" style={{ maxHeight: '400px', overflowY: 'auto' }}>
                <table className="data-table">
                  <thead>
                    <tr>
                      <th>Filename</th>
                      <th>Size</th>
                      <th>Created</th>
                    </tr>
                  </thead>
                  <tbody>
                    {backups.map((backup, idx) => (
                      <tr key={idx}>
                        <td style={{ fontFamily: 'monospace', fontSize: '12px' }}>{backup.filename}</td>
                        <td>{(backup.size_bytes / 1024 / 1024).toFixed(2)} MB</td>
                        <td>{backup.created_at}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            
            <div className="modal-actions" style={{ marginTop: '16px' }}>
              <button 
                className="btn-secondary"
                onClick={() => setShowBackupDialog(false)}
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Flush DB Confirmation Modal */}
      {showFlushConfirm && (
        <div className="modal-overlay">
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => {
                setShowFlushConfirm(false)
                setFlushInput('')
              }}
              title="Close"
            >×</button>
            <h3 style={{ color: '#ff7b72' }}>⚠️ Flush Database Warning</h3>
            
            <div style={{ marginBottom: '16px', padding: '12px', background: 'rgba(255, 123, 114, 0.1)', borderRadius: '6px', border: '1px solid #ff7b72' }}>
              <p style={{ margin: '0 0 8px 0', color: '#c9d1d9', fontWeight: 'bold' }}>
                You are about to flush database DB{currentDb}
              </p>
              <p style={{ margin: '0', color: '#8b949e', fontSize: '13px' }}>
                This will permanently delete all data in this database and cannot be undone!
              </p>
            </div>
            
            <div className="form-group">
              <label>Please enter <strong style={{ color: '#ff7b72' }}>redis</strong> to confirm flush:</label>
              <input
                type="text"
                value={flushInput}
                onChange={(e) => setFlushInput(e.target.value)}
                placeholder="Type redis to confirm"
                className="form-input"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    confirmFlushDb()
                  }
                }}
              />
            </div>
            
            <div className="modal-actions">
              <button 
                className="btn-secondary"
                onClick={() => {
                  setShowFlushConfirm(false)
                  setFlushInput('')
                }}
              >
                Cancel
              </button>
              <button 
                className="btn-primary"
                style={{ backgroundColor: flushInput.toLowerCase() === 'redis' ? '#ff7b72' : undefined }}
                onClick={confirmFlushDb}
                disabled={flushInput.toLowerCase() !== 'redis'}
              >
                Confirm Flush
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
