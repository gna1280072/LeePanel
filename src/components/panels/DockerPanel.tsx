import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

interface DockerStatus {
  installed: boolean
  version: string
  compose_version: string
  running: boolean
}

interface DockerContainer {
  id: string
  name: string
  image: string
  status: string
  state: string
  ports: string
  created: string
}

interface DockerImage {
  id: string
  repository: string
  tag: string
  size: string
  created: string
}

interface DockerPanelProps {
  sessionId: string | null
}

type DockerTab = 'containers' | 'images' | 'mirror'

export default function DockerPanel({ sessionId }: DockerPanelProps) {
  const [status, setStatus] = useState<DockerStatus | null>(null)
  const [statusLoading, setStatusLoading] = useState(true)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')
  const [activeTab, setActiveTab] = useState<DockerTab>('containers')

  // Docker install/uninstall
  const [installing, setInstalling] = useState(false)
  const [useMirror, setUseMirror] = useState(false)
  const [confirmUninstall, setConfirmUninstall] = useState(false)

  // Streaming log for install/uninstall/pull
  const [streamLogs, setStreamLogs] = useState<string[]>([])
  const [streamActive, setStreamActive] = useState(false)
  const streamEndRef = useRef<HTMLDivElement>(null)

  // Containers
  const [containers, setContainers] = useState<DockerContainer[]>([])
  const [containersLoading, setContainersLoading] = useState(false)
  const [containerAction, setContainerAction] = useState('')
  const [logContainer, setLogContainer] = useState<DockerContainer | null>(null)
  const [containerLogs, setContainerLogs] = useState('')
  const [logsLoading, setLogsLoading] = useState(false)
  const [confirmDeleteContainer, setConfirmDeleteContainer] = useState<DockerContainer | null>(null)

  // Images
  const [images, setImages] = useState<DockerImage[]>([])
  const [imagesLoading, setImagesLoading] = useState(false)
  const [pullImageName, setPullImageName] = useState('')
  const [pulling, setPulling] = useState(false)
  const [confirmDeleteImage, setConfirmDeleteImage] = useState<DockerImage | null>(null)
  const [runImageModal, setRunImageModal] = useState<DockerImage | null>(null)
  const [runCommand, setRunCommand] = useState('')
  const [runningContainer, setRunningContainer] = useState(false)

  // Mirror config
  const [mirrors, setMirrors] = useState<string[]>([])
  const [mirrorInput, setMirrorInput] = useState('')
  const [mirrorLoading, setMirrorLoading] = useState(false)
  const [mirrorSaving, setMirrorSaving] = useState(false)

  const fetchStatus = useCallback(async () => {
    if (!sessionId) return
    setStatusLoading(true)
    try {
      const s = await invoke<DockerStatus>('server_check_docker', { sessionId })
      setStatus(s)
    } catch (e) {
      setError(String(e))
    } finally {
      setStatusLoading(false)
    }
  }, [sessionId])

  const fetchContainers = useCallback(async () => {
    if (!sessionId) return
    setContainersLoading(true)
    try {
      const list = await invoke<DockerContainer[]>('server_docker_container_list', { sessionId })
      setContainers(list)
    } catch (e) {
      setError(String(e))
    } finally {
      setContainersLoading(false)
    }
  }, [sessionId])

  const fetchImages = useCallback(async () => {
    if (!sessionId) return
    setImagesLoading(true)
    try {
      const list = await invoke<DockerImage[]>('server_docker_image_list', { sessionId })
      setImages(list)
    } catch (e) {
      setError(String(e))
    } finally {
      setImagesLoading(false)
    }
  }, [sessionId])

  useEffect(() => { fetchStatus() }, [fetchStatus])

  // Listen for docker-action-progress events
  useEffect(() => {
    const unlisten = listen<{ sessionId: string; line: string; status: string }>('docker-action-progress', (event) => {
      if (event.payload.sessionId !== sessionId) return
      setStreamLogs(prev => [...prev, event.payload.line])
      if (event.payload.status === 'done' || event.payload.status === 'error') {
        setStreamActive(false)
      }
    })
    return () => { unlisten.then(fn => fn()) }
  }, [sessionId])

  // Auto-scroll stream log
  useEffect(() => {
    streamEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [streamLogs])

  useEffect(() => {
    if (status?.installed && status?.running) {
      fetchContainers()
      fetchImages()
      fetchMirrorConfig()
    }
  }, [status?.installed, status?.running, fetchContainers, fetchImages])

  const fetchMirrorConfig = useCallback(async () => {
    if (!sessionId) return
    setMirrorLoading(true)
    try {
      const list = await invoke<string[]>('server_docker_get_mirror_config', { sessionId })
      setMirrors(list)
    } catch {
      setMirrors([])
    } finally {
      setMirrorLoading(false)
    }
  }, [sessionId])

  const handleSaveMirror = async () => {
    if (!sessionId || !mirrorInput.trim()) return
    clearMessages()
    setMirrorSaving(true)
    const newMirrors = mirrorInput.split('\n').map(s => s.trim()).filter(Boolean)
    try {
      const result = await invoke<string>('server_docker_set_mirror_config', { sessionId, mirrors: newMirrors })
      setSuccess(result)
      setMirrors(newMirrors)
      setMirrorInput('')
    } catch (e) {
      setError(String(e))
    } finally {
      setMirrorSaving(false)
    }
  }

  const handleRemoveMirror = async (url: string) => {
    if (!sessionId) return
    clearMessages()
    const newMirrors = mirrors.filter(m => m !== url)
    setMirrorSaving(true)
    try {
      const result = await invoke<string>('server_docker_set_mirror_config', { sessionId, mirrors: newMirrors })
      setSuccess(result)
      setMirrors(newMirrors)
    } catch (e) {
      setError(String(e))
    } finally {
      setMirrorSaving(false)
    }
  }

  const clearMessages = () => { setError(''); setSuccess('') }

  const startStream = () => {
    setStreamLogs([])
    setStreamActive(true)
  }

  const handleInstall = async () => {
    if (!sessionId) return
    clearMessages()
    startStream()
    setInstalling(true)
    try {
      await invoke<string>('server_install_docker', { sessionId, useMirror })
      await fetchStatus()
    } catch (e) {
      setError(String(e))
    } finally {
      setInstalling(false)
    }
  }

  const handleUninstall = async () => {
    if (!sessionId) return
    clearMessages()
    startStream()
    setConfirmUninstall(false)
    setInstalling(true)
    try {
      await invoke<string>('server_uninstall_docker', { sessionId })
      await fetchStatus()
    } catch (e) {
      setError(String(e))
    } finally {
      setInstalling(false)
    }
  }

  const handleContainerAction = async (container: DockerContainer, action: string) => {
    if (!sessionId) return
    clearMessages()
    setContainerAction(container.id + action)
    try {
      await invoke('server_docker_container_action', { sessionId, containerId: container.id, action })
      await fetchContainers()
    } catch (e) {
      setError(String(e))
    } finally {
      setContainerAction('')
    }
  }

  const handleDeleteContainer = async (container: DockerContainer, force: boolean) => {
    if (!sessionId) return
    clearMessages()
    setConfirmDeleteContainer(null)
    setContainerAction(container.id + 'delete')
    try {
      await invoke('server_docker_container_remove', { sessionId, containerId: container.id, force })
      await fetchContainers()
    } catch (e) {
      setError(String(e))
    } finally {
      setContainerAction('')
    }
  }

  const handleViewLogs = async (container: DockerContainer) => {
    if (!sessionId) return
    setLogContainer(container)
    setContainerLogs('')
    setLogsLoading(true)
    try {
      const logs = await invoke<string>('server_docker_container_logs', { sessionId, containerId: container.id, lines: 500 })
      setContainerLogs(logs)
    } catch (e) {
      setContainerLogs('Error: ' + String(e))
    } finally {
      setLogsLoading(false)
    }
  }

  const handlePullImage = async () => {
    if (!sessionId || !pullImageName.trim()) return
    clearMessages()
    startStream()
    setPulling(true)
    try {
      await invoke<string>('server_docker_image_pull', { sessionId, imageName: pullImageName.trim() })
      setPullImageName('')
      await fetchImages()
    } catch (e) {
      setError(String(e))
    } finally {
      setPulling(false)
    }
  }

  const handleDeleteImage = async (image: DockerImage) => {
    if (!sessionId) return
    clearMessages()
    setConfirmDeleteImage(null)
    try {
      const imageRef = image.repository === '<none>' ? image.id : `${image.repository}:${image.tag}`
      await invoke('server_docker_image_remove', { sessionId, imageId: imageRef })
      await fetchImages()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleRunFromImage = (image: DockerImage) => {
    setRunImageModal(image)
    // ponytail: provide sensible defaults based on common patterns
    setRunCommand(`-p 80:80 -d`)
  }

  const handleExecuteRun = async () => {
    if (!sessionId || !runImageModal) return
    clearMessages()
    setRunningContainer(true)
    const imageName = runImageModal.repository === '<none>' ? runImageModal.id : `${runImageModal.repository}:${runImageModal.tag}`
    try {
      await invoke('server_docker_image_run', { 
        sessionId, 
        imageName, 
        runArgs: runCommand.trim() 
      })
      setRunImageModal(null)
      setRunCommand('')
      await fetchContainers()
      await fetchImages()
    } catch (e) {
      setError(String(e))
    } finally {
      setRunningContainer(false)
    }
  }

  const getStateClass = (state: string) => {
    switch (state.toLowerCase()) {
      case 'running': return 'docker-state-running'
      case 'exited': return 'docker-state-exited'
      case 'paused': return 'docker-state-paused'
      case 'restarting': return 'docker-state-restarting'
      default: return 'docker-state-unknown'
    }
  }

  if (!sessionId) return <div className="sp-empty">Connect to a server first</div>

  return (
    <div className="docker-panel">
      <div className="docker-header">
        <h2>Docker</h2>
        <button className="docker-refresh-btn" onClick={() => { fetchStatus(); if (status?.installed) { fetchContainers(); fetchImages() } }} disabled={statusLoading}>
          {statusLoading ? '...' : '↻ Refresh'}
        </button>
      </div>

      {error && <div className="docker-message docker-error">{error}</div>}
      {success && <div className="docker-message docker-success">{success}</div>}

      {/* Docker Status Card */}
      <div className="docker-status-card">
        {statusLoading && !status ? (
          <div className="docker-status-loading">Checking Docker...</div>
        ) : status ? (
          <>
            <div className="docker-status-info">
              <span className={`docker-status-badge ${status.installed && status.running ? 'active' : status.installed ? 'installed' : 'not-installed'}`}>
                {status.installed
                  ? status.running ? 'Running' : 'Installed (Stopped)'
                  : 'Not Installed'}
              </span>
              {status.installed && (
                <>
                  <span className="docker-version">Docker {status.version || 'unknown'}</span>
                  {status.compose_version && <span className="docker-version">Compose {status.compose_version}</span>}
                </>
              )}
            </div>
            <div className="docker-status-actions">
              {!status.installed ? (
                <div className="docker-install-section">
                  <label className="docker-mirror-checkbox">
                    <input type="checkbox" checked={useMirror} onChange={(e) => setUseMirror(e.target.checked)} />
                    Use China Mirror (Aliyun)
                  </label>
                  <button className="docker-btn primary" onClick={handleInstall} disabled={installing}>
                    {installing ? 'Installing...' : 'Install Docker'}
                  </button>
                </div>
              ) : (
                <button className="docker-btn danger" onClick={() => setConfirmUninstall(true)} disabled={installing}>
                  Uninstall
                </button>
              )}
            </div>
          </>
        ) : null}
      </div>

      {/* Streaming Log Panel */}
      {(streamActive || streamLogs.length > 0) && (
        <div className="docker-stream-panel">
          <div className="docker-stream-header">
            <span className="docker-stream-title">
              {streamActive ? '⟳ Running...' : '✓ Completed'}
            </span>
            {streamLogs.length > 0 && (
              <button className="docker-stream-clear" onClick={() => setStreamLogs([])}>✕ Clear</button>
            )}
          </div>
          <div className="docker-stream-body">
            {streamLogs.map((line, i) => (
              <div key={i} className="docker-stream-line">{line}</div>
            ))}
            <div ref={streamEndRef} />
          </div>
        </div>
      )}

      {/* Tabs - only show if Docker is installed */}
      {status?.installed && (
        <>
          <div className="docker-tabs">
            <button className={`docker-tab ${activeTab === 'containers' ? 'active' : ''}`} onClick={() => setActiveTab('containers')}>
              Containers ({containers.length})
            </button>
            <button className={`docker-tab ${activeTab === 'images' ? 'active' : ''}`} onClick={() => setActiveTab('images')}>
              Images ({images.length})
            </button>
            <button className={`docker-tab ${activeTab === 'mirror' ? 'active' : ''}`} onClick={() => setActiveTab('mirror')}>
              Mirror
            </button>
          </div>

          {/* Containers Tab */}
          {activeTab === 'containers' && (
            <div className="docker-tab-content">
              {containersLoading && containers.length === 0 ? (
                <div className="docker-loading">Loading containers...</div>
              ) : containers.length === 0 ? (
                <div className="docker-empty">No containers found.</div>
              ) : (
                <div className="docker-table">
                  <div className="docker-table-header">
                    <span className="docker-col-name">Name</span>
                    <span className="docker-col-image">Image</span>
                    <span className="docker-col-status">Status</span>
                    <span className="docker-col-ports">Ports</span>
                    <span className="docker-col-actions">Actions</span>
                  </div>
                  {containers.map((c) => (
                    <div className="docker-table-row" key={c.id}>
                      <span className="docker-col-name" title={c.name}>{c.name}</span>
                      <span className="docker-col-image" title={c.image}>{c.image}</span>
                      <span className={`docker-col-status ${getStateClass(c.state)}`}>{c.status}</span>
                      <span className="docker-col-ports" title={c.ports}>{c.ports || '-'}</span>
                      <span className="docker-col-actions">
                        {c.state === 'running' ? (
                          <>
                            <button className="docker-action-btn" onClick={() => handleContainerAction(c, 'stop')} disabled={!!containerAction} title="Stop">⏹</button>
                            <button className="docker-action-btn" onClick={() => handleContainerAction(c, 'restart')} disabled={!!containerAction} title="Restart">🔄</button>
                            <button className="docker-action-btn" onClick={() => handleContainerAction(c, 'pause')} disabled={!!containerAction} title="Pause">⏸</button>
                          </>
                        ) : c.state === 'paused' ? (
                          <button className="docker-action-btn" onClick={() => handleContainerAction(c, 'unpause')} disabled={!!containerAction} title="Unpause">▶</button>
                        ) : (
                          <button className="docker-action-btn" onClick={() => handleContainerAction(c, 'start')} disabled={!!containerAction} title="Start">▶</button>
                        )}
                        <button className="docker-action-btn" onClick={() => handleViewLogs(c)} disabled={!!containerAction} title="Logs">📋</button>
                        <button className="docker-action-btn danger" onClick={() => setConfirmDeleteContainer(c)} disabled={!!containerAction} title="Delete">🗑</button>
                        {containerAction === c.id + 'stop' || containerAction === c.id + 'start' || containerAction === c.id + 'restart' || containerAction === c.id + 'pause' || containerAction === c.id + 'unpause' || containerAction === c.id + 'delete' ? (
                          <span className="docker-action-loading">...</span>
                        ) : null}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Images Tab */}
          {activeTab === 'images' && (
            <div className="docker-tab-content">
              <div className="docker-pull-section">
                <input
                  className="docker-pull-input"
                  value={pullImageName}
                  onChange={(e) => setPullImageName(e.target.value)}
                  placeholder="nginx:latest, redis:7, mysql:8.0..."
                  onKeyDown={(e) => { if (e.key === 'Enter') handlePullImage() }}
                  disabled={pulling}
                />
                <button className="docker-btn primary" onClick={handlePullImage} disabled={pulling || !pullImageName.trim()}>
                  {pulling ? 'Pulling...' : 'Pull Image'}
                </button>
              </div>

              {imagesLoading && images.length === 0 ? (
                <div className="docker-loading">Loading images...</div>
              ) : images.length === 0 ? (
                <div className="docker-empty">No images found. Pull an image to get started.</div>
              ) : (
                <div className="docker-table">
                  <div className="docker-table-header">
                    <span className="docker-col-repo">Repository</span>
                    <span className="docker-col-tag">Tag</span>
                    <span className="docker-col-id">Image ID</span>
                    <span className="docker-col-size">Size</span>
                    <span className="docker-col-actions">Actions</span>
                  </div>
                  {images.map((img, idx) => (
                    <div className="docker-table-row" key={`${img.id}-${idx}`}>
                      <span className="docker-col-repo">{img.repository}</span>
                      <span className="docker-col-tag">{img.tag}</span>
                      <span className="docker-col-id">{img.id.substring(0, 12)}</span>
                      <span className="docker-col-size">{img.size}</span>
                      <span className="docker-col-actions">
                        <button className="docker-action-btn" onClick={() => handleRunFromImage(img)} title="Run Container">▶️</button>
                        <button className="docker-action-btn danger" onClick={() => setConfirmDeleteImage(img)} title="Delete">🗑</button>
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Mirror Tab */}
          {activeTab === 'mirror' && (
            <div className="docker-tab-content">
              <div className="docker-mirror-section">
                <div className="docker-mirror-header">
                  <h3>Registry Mirrors</h3>
                  <p className="docker-mirror-desc">Configure Docker registry mirrors to accelerate image pulls (especially useful in China).</p>
                </div>

                {mirrorLoading ? (
                  <div className="docker-loading">Loading config...</div>
                ) : (
                  <>
                    {mirrors.length > 0 && (
                      <div className="docker-mirror-current">
                        <span className="docker-mirror-label">Current mirrors:</span>
                        {mirrors.map((m, i) => (
                          <span key={i} className="docker-mirror-tag">
                            {m}
                            <button className="docker-mirror-remove" onClick={() => handleRemoveMirror(m)} title="Remove">✕</button>
                          </span>
                        ))}
                      </div>
                    )}

                    <div className="docker-mirror-form">
                      <label className="docker-mirror-form-label">
                        Mirror URLs (one per line):
                      </label>
                      <textarea
                        className="docker-mirror-textarea"
                        value={mirrorInput}
                        onChange={(e) => setMirrorInput(e.target.value)}
                        placeholder={"https://mirror.ccs.tencentyun.com\nhttps://registry.docker-cn.com"}
                        rows={4}
                      />
                      <div className="docker-mirror-actions">
                        <button className="docker-btn primary" onClick={handleSaveMirror} disabled={mirrorSaving || !mirrorInput.trim()}>
                          {mirrorSaving ? 'Saving...' : 'Save & Restart Docker'}
                        </button>
                        <button className="docker-btn" onClick={() => { setMirrorInput(mirrors.join('\n')) }}>
                          Load Current
                        </button>
                      </div>
                    </div>

                    <div className="docker-mirror-presets">
                      <span className="docker-mirror-presets-title">Common mirrors:</span>
                      <div className="docker-mirror-presets-list">
                        {[
                          { name: 'Aliyun', url: 'https://registry.cn-hangzhou.aliyuncs.com' },
                          { name: 'Tencent', url: 'https://mirror.ccs.tencentyun.com' },
                          { name: 'DaoCloud', url: 'https://f1361db2.m.daocloud.io' },
                          { name: 'Docker CN', url: 'https://registry.docker-cn.com' },
                        ].map((preset) => (
                          <button
                            key={preset.name}
                            className="docker-mirror-preset-btn"
                            onClick={() => setMirrorInput(prev => {
                              const lines = prev.split('\n').filter(Boolean)
                              if (lines.includes(preset.url)) return prev
                              return [...lines, preset.url].join('\n')
                            })}
                          >
                            + {preset.name}
                          </button>
                        ))}
                      </div>
                    </div>
                  </>
                )}
              </div>
            </div>
          )}
        </>
      )}

      {/* Container Logs Modal */}
      {logContainer && (
        <div className="docker-modal-overlay">
          <div className="docker-modal" onClick={(e) => e.stopPropagation()}>
            <div className="docker-modal-header">
              <span className="docker-modal-title">Logs: {logContainer.name}</span>
              <button className="docker-modal-close" onClick={() => setLogContainer(null)}>×</button>
            </div>
            <div className="docker-modal-body">
              {logsLoading ? (
                <div className="docker-loading">Loading logs...</div>
              ) : (
                <pre className="docker-logs-content">{containerLogs || 'No logs available.'}</pre>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Confirm Uninstall Dialog */}
      {confirmUninstall && (
        <div className="docker-modal-overlay">
          <div className="docker-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setConfirmUninstall(false)}
              title="关闭"
            >×</button>
            <div className="docker-confirm-title">Uninstall Docker</div>
            <div className="docker-confirm-msg">
              This will stop all containers and remove Docker. Images and volumes in <code>/var/lib/docker</code> will be preserved.
            </div>
            <div className="docker-confirm-actions">
              <button className="docker-btn" onClick={() => setConfirmUninstall(false)}>Cancel</button>
              <button className="docker-btn danger" onClick={handleUninstall}>Uninstall</button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm Delete Container Dialog */}
      {confirmDeleteContainer && (
        <div className="docker-modal-overlay">
          <div className="docker-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setConfirmDeleteContainer(null)}
              title="关闭"
            >×</button>
            <div className="docker-confirm-title">Delete Container</div>
            <div className="docker-confirm-msg">
              Delete container <strong>{confirmDeleteContainer.name}</strong>?
              {confirmDeleteContainer.state === 'running' && <span className="docker-warn"> (Running container will be force-removed)</span>}
            </div>
            <div className="docker-confirm-actions">
              <button className="docker-btn" onClick={() => setConfirmDeleteContainer(null)}>Cancel</button>
              <button className="docker-btn danger" onClick={() => handleDeleteContainer(confirmDeleteContainer, confirmDeleteContainer.state === 'running')}>
                Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Confirm Delete Image Dialog */}
      {confirmDeleteImage && (
        <div className="docker-modal-overlay">
          <div className="docker-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => setConfirmDeleteImage(null)}
              title="关闭"
            >×</button>
            <div className="docker-confirm-title">Delete Image</div>
            <div className="docker-confirm-msg">
              Delete image <strong>{confirmDeleteImage.repository === '<none>' ? confirmDeleteImage.id.substring(0, 12) : `${confirmDeleteImage.repository}:${confirmDeleteImage.tag}`}</strong>?
            </div>
            <div className="docker-confirm-actions">
              <button className="docker-btn" onClick={() => setConfirmDeleteImage(null)}>Cancel</button>
              <button className="docker-btn danger" onClick={() => handleDeleteImage(confirmDeleteImage)}>Delete</button>
            </div>
          </div>
        </div>
      )}

      {/* Run Container Modal */}
      {runImageModal && (
        <div className="docker-modal-overlay">
          <div className="docker-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <button 
              className="modal-close-btn"
              onClick={() => {
                setRunImageModal(null)
                setRunCommand('')
              }}
              title="关闭"
            >×</button>
            <div className="docker-confirm-title">
              Run Container: {runImageModal.repository === '<none>' ? runImageModal.id.substring(0, 12) : `${runImageModal.repository}:${runImageModal.tag}`}
            </div>
            <div className="docker-confirm-msg">
              Enter docker run arguments (without 'docker run' and image name):
            </div>
            <textarea
              className="docker-mirror-textarea"
              value={runCommand}
              onChange={(e) => setRunCommand(e.target.value)}
              placeholder="-p 80:80 -d --name mycontainer"
              rows={3}
              style={{ marginTop: '12px', marginBottom: '12px' }}
            />
            <div className="docker-confirm-actions">
              <button className="docker-btn" onClick={() => {
                setRunImageModal(null)
                setRunCommand('')
              }}>Cancel</button>
              <button className="docker-btn primary" onClick={handleExecuteRun} disabled={runningContainer}>
                {runningContainer ? 'Running...' : 'Run Container'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
