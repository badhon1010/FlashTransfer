import { useState, useCallback } from 'react'
import type { FC, DragEvent } from 'react'
import { open } from '@tauri-apps/plugin-dialog'

export interface SelectedFile {
  /** Absolute OS path — required for the backend to open the file. */
  path: string
  name: string
  size: number
}

interface DropZoneProps {
  selectedDevice: string | null
  onFilesSelected: (files: SelectedFile[]) => void
  onSend: () => void
  pendingFiles: SelectedFile[]
  onClearFiles: () => void
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`
}

function getFileIcon(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (['jpg','jpeg','png','gif','webp','svg','avif'].includes(ext)) return '🖼'
  if (['mp4','mov','avi','mkv','webm'].includes(ext)) return '🎬'
  if (['mp3','flac','wav','aac','ogg'].includes(ext)) return '🎵'
  if (ext === 'pdf') return '📄'
  if (['zip','tar','gz','7z','rar'].includes(ext)) return '📦'
  if (['txt','md','csv','json','xml','yaml'].includes(ext)) return '📝'
  if (['exe','msi','dmg','deb','AppImage'].includes(ext)) return '⚙️'
  return '📁'
}

const DropZone: FC<DropZoneProps> = ({
  selectedDevice,
  onFilesSelected,
  onSend,
  pendingFiles,
  onClearFiles,
}) => {
  const [isDragOver, setIsDragOver] = useState(false)

  /** Open the native OS file picker (via tauri-plugin-dialog). */
  const handleBrowse = useCallback(async () => {
    const result = await open({
      multiple: true,
      directory: false,
    })
    if (!result) return

    const paths = Array.isArray(result) ? result : [result]
    const files: SelectedFile[] = paths.map((p) => {
      const name = p.split(/[/\\]/).pop() ?? p
      return { path: p, name, size: 0 } // size resolved by Rust
    })
    onFilesSelected(files)
  }, [onFilesSelected])

  /** Open the native OS folder picker. */
  const handleBrowseFolder = useCallback(async () => {
    const result = await open({
      multiple: false,
      directory: true,
    })
    if (!result) return

    const paths = Array.isArray(result) ? result : [result]
    const files: SelectedFile[] = paths.map((p) => {
      const name = p.split(/[/\\]/).pop() ?? p
      return { path: p, name, size: 0 } // size resolved by Rust
    })
    onFilesSelected(files)
  }, [onFilesSelected])

  /** Handle Tauri drag-drop — the event payload contains real OS paths. */
  const handleDragOver = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    setIsDragOver(true)
  }
  const handleDragLeave = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    setIsDragOver(false)
  }
  const handleDrop = useCallback(
    async (e: DragEvent<HTMLDivElement>) => {
      e.preventDefault()
      setIsDragOver(false)
      // In the Tauri WebView, DataTransfer items carry real file paths via the
      // webkitGetAsEntry API — we read the name and pass path via Tauri internals.
      // For full path access we rely on the tauri://drag-drop event in App.tsx.
      // Here we just show file names immediately for UX responsiveness.
      const rawFiles = Array.from(e.dataTransfer.files)
      const files: SelectedFile[] = rawFiles.map((f) => ({
        // In Tauri, (f as any).path gives the real OS path in the drag event
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        path: (f as unknown as { path?: string }).path ?? f.name,
        name: f.name,
        size: f.size,
      }))
      onFilesSelected(files)
    },
    [onFilesSelected],
  )

  const totalSize = pendingFiles.reduce((acc, f) => acc + f.size, 0)

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', height: '100%' }}>
      {/* ── Drop target ─────────────────────────────────────────────────── */}
      <div
        id="drop-zone"
        role="button"
        tabIndex={0}
        aria-label="Drop files here or click to browse"
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={handleBrowse}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') void handleBrowse() }}
        style={{
          flex: pendingFiles.length > 0 ? '0 0 auto' : 1,
          minHeight: pendingFiles.length > 0 ? '120px' : '200px',
          borderRadius: 'var(--r-xl)',
          border: `2px dashed ${isDragOver ? 'var(--accent)' : 'var(--border)'}`,
          background: isDragOver ? 'var(--accent-dim)' : 'var(--bg-panel)',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: '10px',
          cursor: 'pointer',
          transition: 'all var(--t-base)',
          position: 'relative',
          overflow: 'hidden',
          boxShadow: isDragOver ? 'var(--accent-glow)' : 'none',
        }}
      >
        {/* Animated dashed border SVG */}
        <svg
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            opacity: isDragOver ? 0.6 : 0.2,
            pointerEvents: 'none',
          }}
          aria-hidden="true"
        >
          <rect
            x="1" y="1"
            width="calc(100% - 2px)"
            height="calc(100% - 2px)"
            rx="18"
            fill="none"
            stroke="var(--accent)"
            strokeWidth="1.5"
            strokeDasharray="10 6"
            style={isDragOver ? { animation: 'dash-flow 0.4s linear infinite' } : {}}
          />
        </svg>

        {/* Upload icon */}
        <div
          style={{
            width: 48, height: 48,
            borderRadius: 'var(--r-lg)',
            background: isDragOver ? 'var(--accent-dim)' : 'var(--bg-surface)',
            border: `1px solid ${isDragOver ? 'var(--accent-border)' : 'var(--border)'}`,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            transition: 'all var(--t-base)',
            transform: isDragOver ? 'scale(1.1) translateY(-4px)' : 'scale(1)',
          }}
        >
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"
              stroke={isDragOver ? 'var(--accent)' : 'var(--text-secondary)'}
              strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            <polyline points="17 8 12 3 7 8"
              stroke={isDragOver ? 'var(--accent)' : 'var(--text-secondary)'}
              strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            <line x1="12" y1="3" x2="12" y2="15"
              stroke={isDragOver ? 'var(--accent)' : 'var(--text-secondary)'}
              strokeWidth="2" strokeLinecap="round" />
          </svg>
        </div>

        <div style={{ textAlign: 'center', zIndex: 1 }}>
          <div style={{
            fontSize: '14px', fontWeight: 600,
            color: isDragOver ? 'var(--accent)' : 'var(--text-primary)',
            transition: 'color var(--t-fast)',
          }}>
            {isDragOver ? 'Release to add files' : 'Drop files here'}
          </div>
          <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '4px', display: 'flex', gap: '8px', justifyContent: 'center' }}>
            <span>or <span onClick={(e) => { e.stopPropagation(); void handleBrowse(); }} style={{ color: 'var(--accent)', fontWeight: 500, cursor: 'pointer' }}>browse files</span></span>
            <span>/</span>
            <span onClick={(e) => { e.stopPropagation(); void handleBrowseFolder(); }} style={{ color: 'var(--accent)', fontWeight: 500, cursor: 'pointer' }}>browse folders</span>
          </div>
        </div>
      </div>

      {/* ── Pending files list ───────────────────────────────────────────── */}
      {pendingFiles.length > 0 && (
        <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: '6px' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '4px' }}>
            <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-secondary)' }}>
              {pendingFiles.length} file{pendingFiles.length !== 1 ? 's' : ''}
              {totalSize > 0 && ` — ${formatBytes(totalSize)}`}
            </span>
            <button
              onClick={onClearFiles}
              style={{
                fontSize: '11px', color: 'var(--text-muted)',
                background: 'none', border: 'none', cursor: 'pointer',
                padding: '2px 6px', borderRadius: 'var(--r-sm)', transition: 'color var(--t-fast)',
              }}
              onMouseEnter={(e) => { (e.currentTarget).style.color = 'var(--error)' }}
              onMouseLeave={(e) => { (e.currentTarget).style.color = 'var(--text-muted)' }}
            >
              Clear all
            </button>
          </div>

          {pendingFiles.map((file, i) => (
            <div
              key={`${file.path}-${i}`}
              style={{
                display: 'flex', alignItems: 'center', gap: '10px',
                padding: '8px 12px', borderRadius: 'var(--r-md)',
                background: 'var(--bg-surface)', border: '1px solid var(--border)',
                animation: 'fade-in 150ms ease both',
              }}
            >
              <span style={{ fontSize: '18px', flexShrink: 0 }}>{getFileIcon(file.name)}</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="truncate" style={{ fontSize: '13px', fontWeight: 500, color: 'var(--text-primary)' }}>
                  {file.name}
                </div>
                <div className="truncate" style={{ fontSize: '11px', color: 'var(--text-muted)' }} title={file.path}>
                  {file.size > 0 ? formatBytes(file.size) : file.path}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* ── Send button ──────────────────────────────────────────────────── */}
      {pendingFiles.length > 0 && (
        <button
          id="send-btn"
          onClick={onSend}
          disabled={selectedDevice === null}
          style={{
            padding: '12px', borderRadius: 'var(--r-md)', border: 'none',
            background: selectedDevice
              ? 'linear-gradient(135deg, #00c6ff 0%, #0072ff 100%)'
              : 'var(--bg-surface)',
            color: selectedDevice ? '#fff' : 'var(--text-muted)',
            fontSize: '14px', fontWeight: 600,
            cursor: selectedDevice ? 'pointer' : 'not-allowed',
            transition: 'all var(--t-base)',
            boxShadow: selectedDevice ? 'var(--accent-glow)' : 'none',
            display: 'flex', alignItems: 'center', justifyContent: 'center', gap: '8px',
          }}
          onMouseEnter={(e) => { if (selectedDevice) (e.currentTarget).style.transform = 'translateY(-1px)' }}
          onMouseLeave={(e) => { (e.currentTarget).style.transform = 'none' }}
          aria-label={selectedDevice ? 'Send files to selected device' : 'Select a device first'}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M22 2L11 13M22 2L15 22l-4-9-9-4 20-7z"
              stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {selectedDevice ? 'Send Files' : 'Select a device first'}
        </button>
      )}
    </div>
  )
}

export default DropZone
