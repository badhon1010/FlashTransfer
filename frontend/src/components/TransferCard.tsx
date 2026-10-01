import type { FC } from 'react'

export type TransferStatus = 'pending' | 'transferring' | 'paused' | 'done' | 'error' | 'rejected'

export interface Transfer {
  id: string
  batchName: string
  totalSize: number
  transferred: number
  speed: number // bytes per second
  status: TransferStatus
  direction: 'send' | 'receive'
  deviceName: string
}

interface TransferCardProps {
  transfer: Transfer
  onPause?: (id: string) => void
  onResume?: (id: string) => void
  onCancel?: (id: string) => void
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`
}

function formatEta(remainingBytes: number, speed: number): string {
  if (speed === 0) return '—'
  const seconds = remainingBytes / speed
  if (seconds < 60) return `${Math.ceil(seconds)}s`
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`
  return `${(seconds / 3600).toFixed(1)}h`
}

const statusConfig: Record<
  TransferStatus,
  { label: string; color: string; bgColor: string }
> = {
  pending:     { label: 'Waiting',     color: 'var(--text-muted)',   bgColor: 'transparent' },
  transferring:{ label: 'Transferring',color: 'var(--accent)',       bgColor: 'var(--accent-dim)' },
  paused:      { label: 'Paused',      color: 'var(--warning)',      bgColor: 'rgba(245,158,11,0.1)' },
  done:        { label: 'Done',        color: 'var(--success)',      bgColor: 'var(--success-dim)' },
  error:       { label: 'Error',       color: 'var(--error)',        bgColor: 'var(--error-dim)' },
  rejected:    { label: 'Rejected',    color: 'var(--text-muted)',   bgColor: 'var(--bg-hover)' },
}

const TransferCard: FC<TransferCardProps> = ({ transfer, onPause, onResume, onCancel }) => {
  const { status, batchName, totalSize, transferred, speed, direction, deviceName } = transfer
  const progress = totalSize > 0 ? (transferred / totalSize) * 100 : 0
  const remaining = totalSize - transferred
  const cfg = statusConfig[status]

  return (
    <div
      id={`transfer-${transfer.id}`}
      style={{
        padding: '14px 16px',
        borderRadius: 'var(--r-lg)',
        background: 'var(--bg-surface)',
        border: `1px solid ${status === 'transferring' ? 'var(--border-accent)' : 'var(--border)'}`,
        display: 'flex',
        flexDirection: 'column',
        gap: '10px',
        animation: 'fade-in 200ms ease both',
        transition: 'border-color var(--t-base)',
      }}
    >
      {/* Top row: icon + name + status badge + actions */}
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: '10px' }}>
        {/* Direction indicator */}
        <div
          style={{
            width: 32,
            height: 32,
            borderRadius: 'var(--r-sm)',
            background: 'var(--bg-hover)',
            border: '1px solid var(--border)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
            color: direction === 'send' ? 'var(--accent)' : 'var(--success)',
          }}
        >
          {direction === 'send' ? (
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d="M12 19V5M5 12l7-7 7 7" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          ) : (
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d="M12 5v14M19 12l-7 7-7-7" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          )}
        </div>

        {/* File info */}
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            className="truncate"
            style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-primary)' }}
            title={batchName}
          >
            {batchName}
          </div>
          <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>
            {direction === 'send' ? '↑ To' : '↓ From'} {deviceName} · {formatBytes(totalSize)}
          </div>
        </div>

        {/* Status badge */}
        <span
          style={{
            fontSize: '11px',
            fontWeight: 600,
            padding: '3px 8px',
            borderRadius: 'var(--r-sm)',
            background: cfg.bgColor,
            color: cfg.color,
            border: `1px solid ${cfg.color}30`,
            flexShrink: 0,
            letterSpacing: '0.02em',
          }}
        >
          {cfg.label}
        </span>
      </div>

      {/* Progress bar */}
      {status !== 'done' && status !== 'error' && (
        <div>
          <div
            style={{
              height: 5,
              borderRadius: '99px',
              background: 'var(--bg-hover)',
              overflow: 'hidden',
            }}
            role="progressbar"
            aria-valuenow={Math.round(progress)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={`Transfer progress: ${Math.round(progress)}%`}
          >
            <div
              style={{
                height: '100%',
                width: `${progress}%`,
                borderRadius: '99px',
                background:
                  status === 'paused'
                    ? 'var(--warning)'
                    : status === 'transferring'
                    ? 'linear-gradient(90deg, #00c6ff, #0072ff)'
                    : 'var(--text-muted)',
                transition: 'width 0.5s ease',
                backgroundSize: status === 'transferring' ? '200% 100%' : undefined,
                animation:
                  status === 'transferring' ? 'shimmer 2s linear infinite' : undefined,
              }}
            />
          </div>

          {/* Stats row */}
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              marginTop: '6px',
              fontSize: '11px',
              color: 'var(--text-muted)',
            }}
          >
            <span>
              {formatBytes(transferred)} / {formatBytes(totalSize)}
              {' '}
              <span style={{ fontWeight: 600, color: 'var(--text-secondary)' }}>
                ({Math.round(progress)}%)
              </span>
            </span>
            <span>
              {status === 'transferring' && speed > 0 && (
                <>
                  {formatBytes(speed)}/s · ETA {formatEta(remaining, speed)}
                </>
              )}
              {status === 'paused' && 'Paused'}
              {status === 'pending' && 'Waiting to start…'}
            </span>
          </div>
        </div>
      )}

      {/* Done / Error state */}
      {status === 'done' && (
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '12px', color: 'var(--success)' }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M20 6L9 17l-5-5" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          Transfer complete · {formatBytes(totalSize)}
        </div>
      )}
      {status === 'error' && (
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '12px', color: 'var(--error)' }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="2" />
            <path d="M12 8v4M12 16h.01" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
          </svg>
          Transfer failed — connection lost
        </div>
      )}

      {/* Action buttons */}
      {(status === 'transferring' || status === 'paused' || status === 'error') && (
        <div style={{ display: 'flex', gap: '6px', justifyContent: 'flex-end' }}>
          {status === 'transferring' && onPause && (
            <ActionButton
              id={`pause-${transfer.id}`}
              label="Pause"
              onClick={() => { onPause(transfer.id) }}
              icon={
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <rect x="6" y="4" width="4" height="16" rx="1" fill="currentColor" />
                  <rect x="14" y="4" width="4" height="16" rx="1" fill="currentColor" />
                </svg>
              }
            />
          )}
          {(status === 'paused' || (status === 'error' && direction === 'send')) && onResume && (
            <ActionButton
              id={`resume-${transfer.id}`}
              label={status === 'error' ? 'Retry / Resume' : 'Resume'}
              onClick={() => { onResume(transfer.id) }}
              accent
              icon={
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <polygon points="5,3 19,12 5,21" fill="currentColor" />
                </svg>
              }
            />
          )}
          {onCancel && (
            <ActionButton
              id={`cancel-${transfer.id}`}
              label="Cancel"
              onClick={() => { onCancel(transfer.id) }}
              danger
              icon={
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <path d="M18 6L6 18M6 6l12 12" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                </svg>
              }
            />
          )}
        </div>
      )}
    </div>
  )
}

interface ActionButtonProps {
  id: string
  label: string
  onClick: () => void
  icon: React.ReactNode
  accent?: boolean
  danger?: boolean
}

const ActionButton: FC<ActionButtonProps> = ({ id, label, onClick, icon, accent, danger }) => (
  <button
    id={id}
    onClick={onClick}
    style={{
      display: 'flex',
      alignItems: 'center',
      gap: '5px',
      padding: '5px 10px',
      borderRadius: 'var(--r-sm)',
      border: `1px solid ${danger ? 'rgba(248,113,113,0.3)' : accent ? 'var(--accent-border)' : 'var(--border)'}`,
      background: danger
        ? 'var(--error-dim)'
        : accent
        ? 'var(--accent-dim)'
        : 'var(--bg-hover)',
      color: danger ? 'var(--error)' : accent ? 'var(--accent)' : 'var(--text-secondary)',
      fontSize: '11px',
      fontWeight: 500,
      cursor: 'pointer',
      transition: 'all var(--t-fast)',
    }}
  >
    {icon}
    {label}
  </button>
)

export default TransferCard
