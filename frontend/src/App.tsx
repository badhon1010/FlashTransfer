import { useState, useCallback, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import Header from './components/Header'
import DeviceList from './components/DeviceList'
import type { Device } from './components/DeviceList'
import DropZone from './components/DropZone'
import type { SelectedFile } from './components/DropZone'
import TransferCard from './components/TransferCard'
import type { Transfer, TransferStatus } from './components/TransferCard'
import StatusBar from './components/StatusBar'
import IncomingRequest, { type TransferRequest } from './components/IncomingRequest'
import './App.css'

// ── Types matching Rust serialisation ────────────────────────────────────────

interface DeviceInfo {
  id: string
  name: string
  kind: 'laptop' | 'desktop' | 'phone' | 'tablet' | 'unknown'
  ip: string
  port: number
}

interface LocalInfo {
  name: string
  ip: string
  port: number
}

interface TransferProgress {
  id: string
  batchName: string
  totalSize: number
  transferred: number
  speed: number
  status: TransferStatus
  direction: 'send' | 'receive'
  deviceName: string
}

// ─────────────────────────────────────────────────────────────────────────────

function App() {
  const [devices, setDevices]             = useState<Device[]>([])
  const [selectedDeviceId, setSelectedId] = useState<string | null>(null)
  const [pendingFiles, setPendingFiles]   = useState<SelectedFile[]>([])
  const [transfers, setTransfers]         = useState<Transfer[]>([])
  const [localInfo, setLocalInfo]         = useState<LocalInfo>({ name: 'This PC', ip: '…', port: 0 })

  const [incomingRequests, setIncomingRequests] = useState<TransferRequest[]>([])
  const [theme, setTheme] = useState<'dark' | 'light'>(() => {
    const saved = localStorage.getItem('theme')
    if (saved === 'light' || saved === 'dark') return saved
    return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
  })

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem('theme', theme)
  }, [theme])

  const toggleTheme = useCallback(() => {
    setTheme(prev => prev === 'dark' ? 'light' : 'dark')
  }, [])

  // ── Bootstrap: fetch local info + subscribe to all backend events ──────────
  useEffect(() => {
    let cleanups: Array<() => void> = []

    const bootstrap = async () => {
      // 1. Get local device name + IP
      try {
        const info = await invoke<LocalInfo>('get_local_info')
        setLocalInfo(info)
      } catch (e) {
        console.error('get_local_info failed:', e)
      }

      // 1.5 Load transfer history from DB
      try {
        const history = await invoke<any[]>('get_transfer_history')
        const loadedTransfers: Transfer[] = history.map(h => ({
          id: h.id,
          batchName: h.batch_name,
          totalSize: h.total_size,
          transferred: h.transferred,
          speed: 0,
          status: h.status,
          direction: h.direction,
          deviceName: h.device_name,
        }))
        setTransfers(prev => {
          // Merge with prev, avoiding duplicates (though prev is likely empty on bootstrap)
          const existingIds = new Set(prev.map(t => t.id))
          return [...loadedTransfers.filter(t => !existingIds.has(t.id)), ...prev]
        })
      } catch (e) {
        console.error('get_transfer_history failed:', e)
      }

      // 2. Device discovery events
      const unlistenDiscovered = await listen<DeviceInfo>('device-discovered', ({ payload }) => {
        setDevices((prev) => {
          const exists = prev.some((d) => d.id === payload.id)
          if (exists) return prev
          const device: Device = {
            id:             payload.id,
            name:           payload.name,
            kind:           payload.kind === 'unknown' ? 'laptop' : payload.kind,
            ip:             payload.ip,
            signalStrength: 3, // mDNS doesn't give RSSI; default to full
            isSelected:     false,
          }
          return [...prev, device]
        })
      })
      cleanups.push(unlistenDiscovered)

      const unlistenRemoved = await listen<string>('device-removed', ({ payload }) => {
        setDevices((prev) => prev.filter((d) => d.id !== payload))
        setSelectedId((prev) => (prev === payload ? null : prev))
      })
      cleanups.push(unlistenRemoved)

      // 3. Transfer progress events — upsert into transfer list
      const unlistenProgress = await listen<TransferProgress>('transfer-progress', ({ payload }) => {
        setTransfers((prev) => {
          const idx = prev.findIndex((t) => t.id === payload.id)
          const updated: Transfer = {
            id:          payload.id,
            batchName:   payload.batchName,
            totalSize:   payload.totalSize,
            transferred: payload.transferred,
            speed:       payload.speed,
            status:      payload.status,
            direction:   payload.direction,
            deviceName:  payload.deviceName,
          }
          if (idx >= 0) {
            const copy = [...prev]
            copy[idx] = updated
            return copy
          }
          return [updated, ...prev]
        })
      })
      cleanups.push(unlistenProgress)

      // 4. Transfer request events
      const unlistenRequest = await listen<TransferRequest>('transfer-request', ({ payload }) => {
        setIncomingRequests((prev) => {
          if (prev.some((r) => r.id === payload.id)) return prev
          return [...prev, payload]
        })
      })
      cleanups.push(unlistenRequest)
    }

    void bootstrap()
    return () => { cleanups.forEach((fn) => { fn() }) }
  }, [])

  // ── Device selection ────────────────────────────────────────────────────────
  const handleSelectDevice = useCallback((id: string) => {
    setDevices((prev) => prev.map((d) => ({ ...d, isSelected: d.id === id && !d.isSelected })))
    setSelectedId((prev) => (prev === id ? null : id))
  }, [])

  // ── File selection ──────────────────────────────────────────────────────────
  const handleFilesSelected = useCallback((files: SelectedFile[]) => {
    setPendingFiles((prev) => {
      const existing = new Set(prev.map((f) => f.path))
      return [...prev, ...files.filter((f) => !existing.has(f.path))]
    })
  }, [])

  const handleClearFiles = useCallback(() => { setPendingFiles([]) }, [])

  // ── Send ────────────────────────────────────────────────────────────────────
  const handleSend = useCallback(async () => {
    if (!selectedDeviceId || pendingFiles.length === 0) return
    try {
      await invoke<string[]>('start_transfer', {
        deviceId:  selectedDeviceId,
        filePaths: pendingFiles.map((f) => f.path),
      })
      setPendingFiles([])
    } catch (e) {
      console.error('start_transfer failed:', e)
    }
  }, [selectedDeviceId, pendingFiles])

  // ── Pause / Resume / Cancel ─────────────────────────────────────────────────
  const handlePause = useCallback(async (id: string) => {
    try { await invoke('pause_transfer', { id }) } catch (e) { console.error(e) }
  }, [])

  const handleResume = useCallback(async (id: string) => {
    try { await invoke('resume_transfer', { id }) } catch (e) { console.error(e) }
  }, [])

  const handleCancel = useCallback(async (id: string) => {
    try { await invoke('cancel_transfer', { id }) } catch (e) { console.error(e) }
    setTransfers((prev) => prev.filter((t) => t.id !== id))
  }, [])

  // ── Incoming Requests ───────────────────────────────────────────────────────
  const handleAcceptRequest = useCallback(async (id: string) => {
    try { await invoke('accept_transfer', { id }) } catch (e) { console.error(e) }
    setIncomingRequests((prev) => prev.filter((r) => r.id !== id))
  }, [])

  const handleRejectRequest = useCallback(async (id: string) => {
    try { await invoke('reject_transfer', { id }) } catch (e) { console.error(e) }
    setIncomingRequests((prev) => prev.filter((r) => r.id !== id))
  }, [])

  const selectedDevice = devices.find((d) => d.isSelected) ?? null
  const isScanning     = devices.length === 0

  return (
    <>
      <Header
        hasDevices={devices.length > 0}
        deviceCount={devices.length}
        theme={theme}
        toggleTheme={toggleTheme}
      />

      <DeviceList
        devices={devices}
        isScanning={isScanning}
        onSelectDevice={handleSelectDevice}
      />

      {/* ── Main content ───────────────────────────────────────────────── */}
      <main style={{ gridArea: 'main', display: 'flex', flexDirection: 'column', overflow: 'hidden', background: 'var(--bg-base)' }}>
        {/* Tabs */}
        <div style={{ display: 'flex', gap: '2px', padding: '12px 20px 0', borderBottom: '1px solid var(--border)' }}>
          {['Send Files', 'Active Transfers'].map((tab, i) => {
            const isActive = i === 0
            const activeCount = transfers.filter(
              (t) => t.status === 'transferring' || t.status === 'paused',
            ).length
            return (
              <button
                key={tab}
                id={`tab-${tab.toLowerCase().replace(' ', '-')}`}
                style={{
                  padding: '6px 14px 10px', background: 'none', border: 'none',
                  borderBottom: `2px solid ${isActive ? 'var(--accent)' : 'transparent'}`,
                  color: isActive ? 'var(--accent)' : 'var(--text-muted)',
                  fontSize: '13px', fontWeight: isActive ? 600 : 400,
                  cursor: 'pointer', transition: 'all var(--t-fast)',
                  marginBottom: '-1px', fontFamily: 'var(--font)',
                  display: 'flex', alignItems: 'center', gap: '6px',
                }}
              >
                {tab}
                {tab === 'Active Transfers' && activeCount > 0 && (
                  <span style={{
                    background: 'var(--accent)', color: '#000',
                    fontSize: '10px', fontWeight: 700,
                    borderRadius: '99px', padding: '1px 6px', lineHeight: 1.5,
                  }}>
                    {activeCount}
                  </span>
                )}
              </button>
            )
          })}
        </div>

        {/* Scrollable body */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '20px', display: 'flex', flexDirection: 'column', gap: '20px' }}>
          {/* Selected device callout */}
          {selectedDevice && (
            <div style={{
              display: 'flex', alignItems: 'center', gap: '10px',
              padding: '10px 14px', borderRadius: 'var(--r-md)',
              background: 'var(--accent-dim)', border: '1px solid var(--accent-border)',
              animation: 'fade-in 150ms ease both',
            }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M22 2L11 13M22 2L15 22l-4-9-9-4 20-7z"
                  stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              <span style={{ fontSize: '13px', color: 'var(--accent)', fontWeight: 500 }}>
                Sending to <strong>{selectedDevice.name}</strong> · {selectedDevice.ip}
              </span>
            </div>
          )}

          <DropZone
            selectedDevice={selectedDeviceId}
            onFilesSelected={handleFilesSelected}
            onSend={() => { void handleSend() }}
            pendingFiles={pendingFiles}
            onClearFiles={handleClearFiles}
          />

          {transfers.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <div style={{
                fontSize: '11px', fontWeight: 600, letterSpacing: '0.08em',
                textTransform: 'uppercase', color: 'var(--text-muted)', padding: '0 2px',
              }}>
                Transfers
              </div>
              {transfers.map((t) => (
                <TransferCard
                  key={t.id}
                  transfer={t}
                  onPause={(id) => { void handlePause(id) }}
                  onResume={(id) => { void handleResume(id) }}
                  onCancel={(id) => { void handleCancel(id) }}
                />
              ))}
            </div>
          )}
        </div>
      </main>

      <StatusBar
        localDeviceName={localInfo.name}
        localIp={localInfo.ip}
        appVersion="0.1.0"
      />

      {/* ── Overlays ──────────────────────────────────────────────────────── */}
      {incomingRequests.length > 0 && (
        <div style={{
          position: 'fixed',
          top: '20px',
          right: '20px',
          zIndex: 100,
          display: 'flex',
          flexDirection: 'column',
          gap: '12px',
          pointerEvents: 'none', // let clicks pass through the container
        }}>
          {incomingRequests.map((req) => (
            <IncomingRequest
              key={req.id}
              request={req}
              onAccept={handleAcceptRequest}
              onReject={handleRejectRequest}
            />
          ))}
        </div>
      )}
    </>
  )
}

export default App
