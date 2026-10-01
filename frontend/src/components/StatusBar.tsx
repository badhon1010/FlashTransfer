import type { FC } from 'react'

interface StatusBarProps {
  localDeviceName: string
  localIp: string
  appVersion: string
}

const StatusBar: FC<StatusBarProps> = ({ localDeviceName, localIp, appVersion }) => {
  return (
    <footer
      style={{
        gridArea: 'statusbar',
        background: 'var(--bg-panel)',
        borderTop: '1px solid var(--border)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 16px',
        height: '100%',
      }}
    >
      {/* Left: local device info */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
        <StatusItem
          icon={
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <rect x="3" y="4" width="18" height="13" rx="2" stroke="currentColor" strokeWidth="2" />
              <path d="M1 21h22" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
            </svg>
          }
          label={localDeviceName}
        />
        <StatusItem
          icon={
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="2" />
              <path
                d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"
                stroke="currentColor"
                strokeWidth="2"
              />
              <path d="M2 12h20" stroke="currentColor" strokeWidth="2" />
            </svg>
          }
          label={localIp}
        />
      </div>

      {/* Right: app version */}
      <div style={{ fontSize: '11px', color: 'var(--text-muted)', letterSpacing: '0.02em' }}>
        FlashTransfer v{appVersion}
      </div>
    </footer>
  )
}

const StatusItem: FC<{ icon: React.ReactNode; label: string }> = ({ icon, label }) => (
  <div
    style={{
      display: 'flex',
      alignItems: 'center',
      gap: '5px',
      fontSize: '11px',
      color: 'var(--text-muted)',
    }}
  >
    {icon}
    <span>{label}</span>
  </div>
)

export default StatusBar
