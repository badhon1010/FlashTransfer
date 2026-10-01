import { useEffect, useState } from 'react'

export interface TransferRequest {
  id: string
  batchName: string
  totalSize: number
  fileCount: number
  senderIp: string
  senderName: string
}

interface IncomingRequestProps {
  request: TransferRequest
  onAccept: (id: string) => void
  onReject: (id: string) => void
}

const TIMEOUT_SECONDS = 60

function formatBytes(bytes: number) {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i]
}

export default function IncomingRequest({ request, onAccept, onReject }: IncomingRequestProps) {
  const [timeLeft, setTimeLeft] = useState(TIMEOUT_SECONDS)

  useEffect(() => {
    if (timeLeft <= 0) {
      onReject(request.id)
      return
    }

    const timer = setInterval(() => {
      setTimeLeft((prev) => prev - 1)
    }, 1000)

    return () => clearInterval(timer)
  }, [timeLeft, onReject, request.id])

  const progress = (timeLeft / TIMEOUT_SECONDS) * 100
  const circumference = 2 * Math.PI * 14
  const dashoffset = circumference - (progress / 100) * circumference

  return (
    <div style={{
      width: '320px',
      background: 'var(--bg-glass)',
      backdropFilter: 'blur(12px)',
      WebkitBackdropFilter: 'blur(12px)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--r-lg)',
      padding: '16px',
      display: 'flex',
      flexDirection: 'column',
      gap: '12px',
      boxShadow: 'var(--shadow-lg)',
      animation: 'slide-in-right var(--t-base) forwards',
      pointerEvents: 'auto',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
        <div style={{
          width: '32px',
          height: '32px',
          borderRadius: '50%',
          background: 'var(--accent-dim)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--accent)',
        }}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M22 2L11 13M22 2L15 22l-4-9-9-4 20-7z" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </div>
        <div style={{ flex: 1, overflow: 'hidden' }}>
          <div style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Incoming from</div>
          <div className="truncate" style={{ fontSize: '14px', fontWeight: 600, color: 'var(--text-primary)' }}>
            {request.senderName}
          </div>
        </div>
        {/* Countdown Ring */}
        <div style={{ position: 'relative', width: '32px', height: '32px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <svg width="32" height="32" style={{ transform: 'rotate(-90deg)' }}>
            <circle
              cx="16" cy="16" r="14"
              fill="none"
              stroke="var(--border)"
              strokeWidth="2"
            />
            <circle
              cx="16" cy="16" r="14"
              fill="none"
              stroke="var(--warning)"
              strokeWidth="2"
              strokeDasharray={circumference}
              strokeDashoffset={dashoffset}
              style={{ transition: 'stroke-dashoffset 1s linear' }}
            />
          </svg>
          <div style={{ position: 'absolute', fontSize: '10px', fontWeight: 600, color: 'var(--warning)' }}>
            {timeLeft}
          </div>
        </div>
      </div>

      <div style={{ background: 'var(--bg-panel)', borderRadius: 'var(--r-sm)', padding: '10px 12px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
        <div className="truncate" style={{ fontSize: '13px', fontWeight: 500, color: 'var(--text-primary)' }}>
          {request.batchName} {request.fileCount > 1 ? `(${request.fileCount} files)` : ''}
        </div>
        <div style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
          {formatBytes(request.totalSize)}
        </div>
      </div>

      <div style={{ display: 'flex', gap: '8px', marginTop: '4px' }}>
        <button
          onClick={() => onReject(request.id)}
          style={{
            flex: 1,
            padding: '8px 0',
            background: 'var(--error-dim)',
            color: 'var(--error)',
            border: '1px solid rgba(248, 113, 113, 0.2)',
            borderRadius: 'var(--r-md)',
            fontSize: '13px',
            fontWeight: 600,
            cursor: 'pointer',
            transition: 'all var(--t-fast)',
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.background = 'rgba(248, 113, 113, 0.2)'
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.background = 'var(--error-dim)'
          }}
        >
          Reject
        </button>
        <button
          onClick={() => onAccept(request.id)}
          style={{
            flex: 1,
            padding: '8px 0',
            background: 'var(--success-dim)',
            color: 'var(--success)',
            border: '1px solid rgba(34, 211, 165, 0.2)',
            borderRadius: 'var(--r-md)',
            fontSize: '13px',
            fontWeight: 600,
            cursor: 'pointer',
            transition: 'all var(--t-fast)',
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.background = 'rgba(34, 211, 165, 0.2)'
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.background = 'var(--success-dim)'
          }}
        >
          Accept
        </button>
      </div>
    </div>
  )
}
