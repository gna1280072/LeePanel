import { useEffect, useRef, useImperativeHandle, forwardRef } from 'react'
import { Terminal as XTerminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { ClipboardAddon } from '@xterm/addon-clipboard'
import '@xterm/xterm/css/xterm.css'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

interface TerminalProps {
  sessionId: string | null
  isActive?: boolean
}

export interface TerminalHandle {
  sendCommand: (cmd: string) => void
  clear: () => void
}

export default forwardRef<TerminalHandle, TerminalProps>(function Terminal({ sessionId, isActive }, ref) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<XTerminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const sidRef = useRef(sessionId)
  // ponytail: track selection state ourselves because ClipboardAddon may clear it before onData fires
  const hasSelectionRef = useRef(false)

  useEffect(() => {
    sidRef.current = sessionId
  }, [sessionId])

  // Initialize terminal
  useEffect(() => {
    if (!containerRef.current) return

    const term = new XTerminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: "'Menlo', 'Monaco', 'Liberation Mono', 'DejaVu Sans Mono', 'Courier New', monospace",
      allowProposedApi: true,
      theme: {
        background: '#0d1117',
        foreground: '#c9d1d9',
        cursor: '#58a6ff',
        selectionBackground: '#264f78',
        black: '#0d1117',
        red: '#ff7b72',
        green: '#3fb950',
        yellow: '#d29922',
        blue: '#58a6ff',
        magenta: '#bc8cff',
        cyan: '#39c5cf',
        white: '#c9d1d9',
        brightBlack: '#484f58',
        brightRed: '#ffa198',
        brightGreen: '#56d364',
        brightYellow: '#e3b341',
        brightBlue: '#79c0ff',
        brightMagenta: '#d2a8ff',
        brightCyan: '#56d4dd',
        brightWhite: '#f0f6fc',
      },
      allowTransparency: true,
    })

    const fitAddon = new FitAddon()
    const webLinksAddon = new WebLinksAddon()
    term.loadAddon(fitAddon)
    term.loadAddon(webLinksAddon)
    term.open(containerRef.current)
    // ponytail: block all DECSET/DECRST mouse-tracking sequences so local text selection works
    // Remote shells (bash/tmux/vim) send \e[?1000h etc. which capture mouse events
    const MOUSE_MODES = new Set([9, 1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1015])
    for (const final of ['h', 'l']) {
      term.parser.registerCsiHandler({ final, prefix: '?' }, (params) => {
        const p = Array.isArray(params[0]) ? params[0][0] : params[0]
        if (MOUSE_MODES.has(p)) return true // block mouse tracking
        return false
      })
    }

    const clipboardAddon = new ClipboardAddon()
    term.loadAddon(clipboardAddon)

    // Track selection state for Ctrl+C copy logic
    term.onSelectionChange(() => {
      hasSelectionRef.current = term.hasSelection()
    })

    // ponytail: sync remote PTY size with xterm.js after every fit
    const syncSize = () => {
      const sid = sidRef.current
      if (sid) {
        invoke('ssh_resize', { sessionId: sid, cols: term.cols, rows: term.rows })
      }
    }
    setTimeout(() => { fitAddon.fit(); syncSize() }, 100)

    term.onData((data) => {
      const sid = sidRef.current
      if (sid) {
        // ponytail: when text is selected, Ctrl+C should copy only (not send interrupt)
        // use our own ref because ClipboardAddon may have cleared term.hasSelection()
        if (data === '\x03' && hasSelectionRef.current) {
          navigator.clipboard.writeText(term.getSelection()).catch(() => {})
          term.clearSelection()
          hasSelectionRef.current = false
          return
        }
        invoke('ssh_input', { sessionId: sid, data })
      }
    })

    termRef.current = term
    fitRef.current = fitAddon

    // Expose sendCommand via ref
    // (done in separate useEffect below)

    // Listen for SSH output
    const unlisten = listen<{ sessionId: string; data: string }>('ssh-output', (event) => {
      const sid = sidRef.current
      if (sid && event.payload.sessionId === sid) {
        term.write(event.payload.data)
      }
    })

    // Handle resize
    const handleResize = () => {
      if (fitRef.current) {
        fitRef.current.fit()
        const sid = sidRef.current
        if (sid) {
          invoke('ssh_resize', {
            sessionId: sid,
            cols: term.cols,
            rows: term.rows,
          })
        }
      }
    }
    window.addEventListener('resize', handleResize)

    // Listen for connection closed
    const unlistenClosed = listen<string>('ssh-closed', (event) => {
      const sid = sidRef.current
      if (sid && event.payload === sid) {
        term.clear()
      }
    })

    return () => {
      unlisten.then((fn) => fn())
      unlistenClosed.then((fn) => fn())
      window.removeEventListener('resize', handleResize)
      term.dispose()
      termRef.current = null
      fitRef.current = null
    }
  }, [])

  useImperativeHandle(ref, () => ({
    sendCommand: (cmd: string) => {
      const sid = sidRef.current
      if (sid && termRef.current) {
        invoke('ssh_input', { sessionId: sid, data: cmd + '\r' })
      }
    },
    clear: () => {
      termRef.current?.clear()
    },
  }))

  // Refit on session change + sync PTY
  useEffect(() => {
    if (fitRef.current && termRef.current) {
      setTimeout(() => {
        fitRef.current?.fit()
        if (sessionId) {
          invoke('ssh_resize', { sessionId, cols: termRef.current!.cols, rows: termRef.current!.rows })
        }
      }, 100)
    }
  }, [sessionId])

  // Refit when tab becomes active (was hidden with display:none) + sync PTY
  useEffect(() => {
    if (isActive && fitRef.current && termRef.current) {
      setTimeout(() => {
        fitRef.current?.fit()
        const sid = sidRef.current
        if (sid) {
          invoke('ssh_resize', { sessionId: sid, cols: termRef.current!.cols, rows: termRef.current!.rows })
        }
        termRef.current?.focus()
      }, 50)
    }
  }, [isActive])

  return (
    <div
      ref={containerRef}
      style={{ width: '100%', height: '100%', background: '#0d1117' }}
    />
  )
})
