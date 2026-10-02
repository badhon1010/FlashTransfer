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

interface TransferRecord {
  id: string
  batchName: string
  totalSize: number
  transferred: number
  status: string
  direction: string
  deviceName: string
  localPath: string
  createdAt: string | null
  updatedAt: string | null
}

interface DeviceHistoryRecord {
  id: string
  name: string
  kind: string
  lastIp: string
  lastSeen: string | null
  transferCount: number
  bytesExchanged: number
}

function fmtBytes(n: number): string {
  if (n === 0) return '0 B'
  const k = 1024
  const units = ['B','KB','MB','GB','TB']
  const i = Math.floor(Math.log(n) / Math.log(k))
  return `${(n / Math.pow(k, i)).toFixed(1)} ${units[i]}`
}

function fmtDate(s: string | null): string {
  if (!s) return '—'
  try {
    return new Date(s + 'Z').toLocaleString()
  } catch { return s }
}

// ─────────────────────────────────────────────────────────────────────────────

function App() {
  const [devices, setDevices]             = useState<Device[]>([])
  const [selectedDeviceId, setSelectedId] = useState<string | null>(null)
  const [pendingFiles, setPendingFiles]   = useState<SelectedFile[]>([])
  const [transfers, setTransfers]         = useState<Transfer[]>([])
  const [localInfo, setLocalInfo]         = useState<LocalInfo>({ name: 'This PC', ip: '…', port: 0 })
  const [currentTab, setCurrentTab]       = useState<'Send Files' | 'Active Transfers' | 'History' | 'Settings'>('Send Files')
  const [historySubTab, setHistorySubTab] = useState<'Transfers' | 'Devices'>('Transfers')
  const [deviceHistory, setDeviceHistory] = useState<DeviceHistoryRecord[]>([])
  const [historyRecords, setHistoryRecords] = useState<TransferRecord[]>([])

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
      const loadHistory = async () => {
        try {
          const raw = await invoke<any[]>('get_transfer_history')
          const records: TransferRecord[] = raw.map(h => ({
            id: h.id, batchName: h.batch_name, totalSize: h.total_size,
            transferred: h.transferred, status: h.status, direction: h.direction,
            deviceName: h.device_name, localPath: h.local_path,
            createdAt: h.created_at ?? null, updatedAt: h.updated_at ?? null,
          }))
          setHistoryRecords(records)
          // Also seed the transfers state for active transfers
          const loadedTransfers: Transfer[] = records.map(h => ({
            id: h.id, batchName: h.batchName, totalSize: h.totalSize,
            transferred: h.transferred, speed: 0, status: h.status as TransferStatus,
            direction: h.direction as 'send' | 'receive', deviceName: h.deviceName,
          }))
          setTransfers(prev => {
            const existingIds = new Set(prev.map(t => t.id))
            return [...loadedTransfers.filter(t => !existingIds.has(t.id)), ...prev]
          })
        } catch (e) { console.error('get_transfer_history failed:', e) }

        try {
          const raw = await invoke<any[]>('get_device_history')
          setDeviceHistory(raw.map(d => ({
            id: d.id, name: d.name, kind: d.kind, lastIp: d.last_ip,
            lastSeen: d.last_seen ?? null, transferCount: d.transfer_count,
            bytesExchanged: d.bytes_exchanged,
          })))
        } catch (e) { console.error('get_device_history failed:', e) }
      }
      await loadHistory()

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

  // ── History actions ─────────────────────────────────────────────────────────
  const handleDeleteTransfer = useCallback(async (id: string) => {
    try { await invoke('delete_transfer', { id }) } catch (e) { console.error(e) }
    setHistoryRecords(prev => prev.filter(r => r.id !== id))
    setTransfers(prev => prev.filter(t => t.id !== id))
  }, [])

  const handleClearHistory = useCallback(async () => {
    try { await invoke('clear_transfer_history') } catch (e) { console.error(e) }
    setHistoryRecords(prev => prev.filter(r => r.status === 'transferring' || r.status === 'paused'))
    setTransfers(prev => prev.filter(t => t.status === 'transferring' || t.status === 'paused'))
  }, [])

  const handleForgetDevice = useCallback(async (id: string) => {
    try { await invoke('forget_device', { id }) } catch (e) { console.error(e) }
    setDeviceHistory(prev => prev.filter(d => d.id !== id))
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
          {(['Send Files', 'Active Transfers', 'History', 'Settings'] as const).map((tab) => {
            const isActive = currentTab === tab
            const activeCount = transfers.filter(
              (t) => t.status === 'transferring' || t.status === 'paused',
            ).length
            return (
              <button
                key={tab}
                id={`tab-${tab.toLowerCase().replace(' ', '-')}`}
                onClick={() => setCurrentTab(tab)}
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
          
          {currentTab === 'Send Files' && (
            <>
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
            </>
          )}

          {currentTab === 'Active Transfers' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <div style={{
                fontSize: '11px', fontWeight: 600, letterSpacing: '0.08em',
                textTransform: 'uppercase', color: 'var(--text-muted)', padding: '0 2px',
              }}>
                Active Transfers
              </div>
              {transfers.filter(t => t.status === 'transferring' || t.status === 'paused').length === 0 && (
                <div style={{ color: 'var(--text-muted)', fontSize: '13px', padding: '20px 0', textAlign: 'center' }}>
                  No active transfers.
                </div>
              )}
              {transfers
                .filter(t => t.status === 'transferring' || t.status === 'paused')
                .map((t) => (
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

          {currentTab === 'History' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
              {/* Sub-tabs */}
              <div style={{ display: 'flex', gap: '2px', borderBottom: '1px solid var(--border)', marginBottom: '4px' }}>
                {(['Transfers', 'Devices'] as const).map(sub => (
                  <button key={sub} onClick={() => setHistorySubTab(sub)} style={{
                    padding: '5px 12px 9px', background: 'none', border: 'none',
                    borderBottom: `2px solid ${historySubTab === sub ? 'var(--accent)' : 'transparent'}`,
                    color: historySubTab === sub ? 'var(--accent)' : 'var(--text-muted)',
                    fontSize: '12px', fontWeight: historySubTab === sub ? 600 : 400,
                    cursor: 'pointer', fontFamily: 'var(--font)', marginBottom: '-1px',
                  }}>{sub}</button>
                ))}
                {historySubTab === 'Transfers' && historyRecords.some(r => ['done','error','rejected'].includes(r.status)) && (
                  <button onClick={() => { void handleClearHistory() }} style={{
                    marginLeft: 'auto', padding: '4px 12px', background: 'none',
                    border: '1px solid var(--border)', borderRadius: 'var(--r-sm)',
                    color: 'var(--text-muted)', fontSize: '11px', cursor: 'pointer',
                    fontFamily: 'var(--font)', alignSelf: 'center',
                  }}>Clear completed</button>
                )}
              </div>

              {historySubTab === 'Transfers' && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                  {historyRecords.filter(r => r.status !== 'transferring' && r.status !== 'paused').length === 0 && (
                    <div style={{ color: 'var(--text-muted)', fontSize: '13px', padding: '20px 0', textAlign: 'center' }}>No transfer history.</div>
                  )}
                  {historyRecords
                    .filter(r => r.status !== 'transferring' && r.status !== 'paused')
                    .map(r => (
                      <div key={r.id} style={{
                        background: 'var(--surface)', borderRadius: 'var(--r-md)',
                        border: '1px solid var(--border)', padding: '12px 14px',
                        display: 'flex', alignItems: 'center', gap: '12px',
                      }}>
                        {/* Direction icon */}
                        <span style={{ fontSize: '18px' }}>{r.direction === 'send' ? '⬆️' : '⬇️'}</span>
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ fontWeight: 500, fontSize: '13px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{r.batchName}</div>
                          <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>
                            {fmtBytes(r.totalSize)} · {r.deviceName} · {fmtDate(r.createdAt)}
                          </div>
                        </div>
                        <span style={{
                          fontSize: '11px', fontWeight: 600, padding: '2px 8px',
                          borderRadius: '99px',
                          background: r.status === 'done' ? 'rgba(0,200,100,0.15)' : r.status === 'error' ? 'rgba(255,80,80,0.15)' : 'rgba(120,120,120,0.15)',
                          color: r.status === 'done' ? '#00c864' : r.status === 'error' ? '#ff5050' : 'var(--text-muted)',
                        }}>{r.status}</span>
                        <button onClick={() => { void handleDeleteTransfer(r.id) }} title="Remove from history" style={{
                          background: 'none', border: 'none', cursor: 'pointer',
                          color: 'var(--text-muted)', fontSize: '16px', padding: '2px 4px',
                          lineHeight: 1, borderRadius: 'var(--r-sm)',
                        }}>✕</button>
                      </div>
                    ))}
                </div>
              )}

              {historySubTab === 'Devices' && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                  {deviceHistory.length === 0 && (
                    <div style={{ color: 'var(--text-muted)', fontSize: '13px', padding: '20px 0', textAlign: 'center' }}>No device history.</div>
                  )}
                  {deviceHistory.map(d => (
                    <div key={d.id} style={{
                      background: 'var(--surface)', borderRadius: 'var(--r-md)',
                      border: '1px solid var(--border)', padding: '12px 14px',
                      display: 'flex', alignItems: 'center', gap: '12px',
                    }}>
                      <span style={{ fontSize: '20px' }}>💻</span>
                      <div style={{ flex: 1 }}>
                        <div style={{ fontWeight: 500, fontSize: '13px' }}>{d.name}</div>
                        <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>
                          {d.lastIp} · {d.transferCount} transfers · {fmtBytes(d.bytesExchanged)} · Last seen {fmtDate(d.lastSeen)}
                        </div>
                      </div>
                      <button onClick={() => { void handleForgetDevice(d.id) }} title="Forget device" style={{
                        background: 'none', border: 'none', cursor: 'pointer',
                        color: 'var(--text-muted)', fontSize: '16px', padding: '2px 4px',
                        lineHeight: 1, borderRadius: 'var(--r-sm)',
                      }}>✕</button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {currentTab === 'Settings' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '20px' }}>
              <div style={{
                fontSize: '11px', fontWeight: 600, letterSpacing: '0.08em',
                textTransform: 'uppercase', color: 'var(--text-muted)', padding: '0 2px',
              }}>
                Settings
              </div>
              <div style={{ background: 'var(--surface)', padding: '16px', borderRadius: 'var(--r-md)', border: '1px solid var(--border)' }}>
                <h3 style={{ margin: '0 0 10px 0', fontSize: '14px', fontWeight: 500 }}>Device Name</h3>
                <p style={{ margin: '0 0 16px 0', fontSize: '12px', color: 'var(--text-muted)' }}>
                  This name will be visible to other devices on the network.
                </p>
                <div style={{ display: 'flex', gap: '10px' }}>
                  <input
                    type="text"
                    defaultValue={localInfo.name}
                    id="device-name-input"
                    style={{
                      flex: 1, padding: '8px 12px', borderRadius: 'var(--r-sm)',
                      border: '1px solid var(--border)', background: 'var(--bg-base)',
                      color: 'var(--text)', fontFamily: 'var(--font)'
                    }}
                  />
                  <button
                    onClick={async () => {
                      const newName = (document.getElementById('device-name-input') as HTMLInputElement).value
                      if (!newName.trim()) return
                      try {
                        await invoke('set_device_name', { name: newName.trim() })
                        setLocalInfo(prev => ({ ...prev, name: newName.trim() }))
                      } catch (e) {
                        console.error('Failed to save device name:', e)
                      }
                    }}
                    style={{
                      padding: '8px 16px', background: 'var(--accent)', color: '#000',
                      border: 'none', borderRadius: 'var(--r-sm)', cursor: 'pointer',
                      fontWeight: 500, fontFamily: 'var(--font)'
                    }}
                  >
                    Save
                  </button>
                </div>
              </div>
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
