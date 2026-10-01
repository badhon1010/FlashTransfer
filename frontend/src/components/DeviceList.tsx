import { useState } from 'react'
import type { FC } from 'react'

export interface Device {
  id: string
  name: string
  kind: 'laptop' | 'desktop' | 'phone' | 'tablet'
  ip: string
  signalStrength: number // 0–3
  isSelected: boolean
}

interface DeviceListProps {
  devices: Device[]
  isScanning: boolean
  onSelectDevice: (id: string) => void
}

const DeviceIcon: FC<{ kind: Device['kind'] }> = ({ kind }) => {
  if (kind === 'phone') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <rect x="5" y="2" width="14" height="20" rx="3" stroke="currentColor" strokeWidth="2" />
        <circle cx="12" cy="18" r="1" fill="currentColor" />
      </svg>
    )
  }
  if (kind === 'tablet') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <rect x="3" y="2" width="18" height="20" rx="3" stroke="currentColor" strokeWidth="2" />
        <circle cx="12" cy="18" r="1" fill="currentColor" />
      </svg>
    )
  }
  if (kind === 'desktop') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <rect x="2" y="3" width="20" height="13" rx="2" stroke="currentColor" strokeWidth="2" />
        <path d="M8 21h8M12 16v5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
    )
  }
  // laptop (default)
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="3" y="4" width="18" height="13" rx="2" stroke="currentColor" strokeWidth="2" />
      <path d="M1 21h22" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  )
}

const SignalBars: FC<{ strength: number }> = ({ strength }) => (
  <div
    style={{ display: 'flex', alignItems: 'flex-end', gap: '2px', height: '12px' }}
    aria-label={`Signal: ${strength}/3`}
  >
    {[1, 2, 3].map((bar) => (
      <div
        key={bar}
        style={{
          width: '3px',
          height: `${bar * 4}px`,
          borderRadius: '1px',
          background: bar <= strength ? 'var(--accent)' : 'var(--text-muted)',
          transition: 'background var(--t-base)',
        }}
      />
    ))}
  </div>
)

const DeviceList: FC<DeviceListProps> = ({ devices, isScanning, onSelectDevice }) => {
  return (
    <aside
      style={{
        gridArea: 'sidebar',
        background: 'var(--bg-panel)',
        borderRight: '1px solid var(--border)',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {/* Section header */}
      <div
        style={{
          padding: '14px 16px 10px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid var(--border)',
        }}
      >
        <span
          style={{
            fontSize: '11px',
            fontWeight: 600,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'var(--text-muted)',
          }}
        >
          Nearby Devices
        </span>

        {isScanning && (
          <div style={{ display: 'flex', alignItems: 'center', gap: '5px' }}>
            <div
              style={{
                width: 6,
                height: 6,
                borderRadius: '50%',
                background: 'var(--accent)',
                animation: 'pulse-ring 1.4s ease-in-out infinite',
              }}
            />
            <span style={{ fontSize: '11px', color: 'var(--accent)', fontWeight: 500 }}>
              Scanning
            </span>
          </div>
        )}
      </div>

      {/* Device list */}
      <div
        style={{
          flex: 1,
          overflowY: 'auto',
          padding: '8px',
          display: 'flex',
          flexDirection: 'column',
          gap: '4px',
        }}
      >
        {devices.length === 0 ? (
          <EmptyState isScanning={isScanning} />
        ) : (
          devices.map((device) => (
            <DeviceCard
              key={device.id}
              device={device}
              onClick={() => { onSelectDevice(device.id) }}
            />
          ))
        )}
      </div>
    </aside>
  )
}

const DeviceCard: FC<{ device: Device; onClick: () => void }> = ({ device, onClick }) => {
  const [hovered, setHovered] = useState(false)

  return (
    <button
      id={`device-${device.id}`}
      onClick={onClick}
      onMouseEnter={() => { setHovered(true) }}
      onMouseLeave={() => { setHovered(false) }}
      style={{
        width: '100%',
        display: 'flex',
        alignItems: 'center',
        gap: '10px',
        padding: '10px 12px',
        borderRadius: 'var(--r-md)',
        border: device.isSelected
          ? '1px solid var(--accent-border)'
          : '1px solid transparent',
        background: device.isSelected
          ? 'var(--accent-dim)'
          : hovered
          ? 'var(--bg-hover)'
          : 'transparent',
        cursor: 'pointer',
        textAlign: 'left',
        transition: 'all var(--t-fast)',
        animation: 'fade-in 200ms ease both',
      }}
      aria-pressed={device.isSelected}
      aria-label={`Select ${device.name}`}
    >
      {/* Icon container */}
      <div
        style={{
          width: 34,
          height: 34,
          borderRadius: 'var(--r-sm)',
          background: device.isSelected ? 'var(--accent-dim)' : 'var(--bg-surface)',
          border: '1px solid var(--border)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          flexShrink: 0,
          color: device.isSelected ? 'var(--accent)' : 'var(--text-secondary)',
          transition: 'all var(--t-fast)',
        }}
      >
        <DeviceIcon kind={device.kind} />
      </div>

      {/* Device info */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          className="truncate"
          style={{
            fontSize: '13px',
            fontWeight: 500,
            color: device.isSelected ? 'var(--accent)' : 'var(--text-primary)',
            transition: 'color var(--t-fast)',
          }}
        >
          {device.name}
        </div>
        <div
          className="truncate"
          style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}
        >
          {device.ip}
        </div>
      </div>

      {/* Signal */}
      <SignalBars strength={device.signalStrength} />
    </button>
  )
}

const EmptyState: FC<{ isScanning: boolean }> = ({ isScanning }) => (
  <div
    style={{
      flex: 1,
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      padding: '32px 16px',
      gap: '12px',
      color: 'var(--text-muted)',
    }}
  >
    {/* Radar icon */}
    <div style={{ position: 'relative', width: 52, height: 52 }}>
      <svg width="52" height="52" viewBox="0 0 52 52" fill="none" aria-hidden="true">
        <circle cx="26" cy="26" r="23" stroke="var(--border)" strokeWidth="1.5" />
        <circle cx="26" cy="26" r="14" stroke="var(--border)" strokeWidth="1.5" />
        <circle cx="26" cy="26" r="5" stroke="var(--border)" strokeWidth="1.5" />
        {isScanning && (
          <>
            <circle
              cx="26"
              cy="26"
              r="23"
              stroke="var(--accent)"
              strokeWidth="1.5"
              strokeOpacity="0.3"
              style={{ animation: 'pulse-ring 2s ease-in-out infinite' }}
            />
            <line
              x1="26"
              y1="26"
              x2="26"
              y2="3"
              stroke="var(--accent)"
              strokeWidth="1.5"
              strokeOpacity="0.6"
              style={{
                transformOrigin: '26px 26px',
                animation: 'spin 2s linear infinite',
              }}
            />
          </>
        )}
      </svg>
      {/* Scan line overlay */}
      {isScanning && (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            borderRadius: '50%',
            overflow: 'hidden',
            pointerEvents: 'none',
          }}
        >
          <div
            style={{
              position: 'absolute',
              inset: 0,
              background:
                'conic-gradient(from 0deg, transparent 70%, rgba(0,198,255,0.15) 100%)',
              animation: 'spin 2s linear infinite',
            }}
          />
        </div>
      )}
    </div>

    <div style={{ textAlign: 'center' }}>
      <div style={{ fontSize: '13px', fontWeight: 500, color: 'var(--text-secondary)', marginBottom: '4px' }}>
        {isScanning ? 'Scanning network…' : 'No devices found'}
      </div>
      <div style={{ fontSize: '11px', lineHeight: 1.5 }}>
        {isScanning
          ? 'Looking for nearby devices'
          : 'Make sure devices are on the same network'}
      </div>
    </div>
  </div>
)

export default DeviceList
