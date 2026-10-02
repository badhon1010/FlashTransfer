import type { FC } from 'react'

interface HeaderProps {
  hasDevices: boolean
  deviceCount: number
  theme: 'dark' | 'light'
  toggleTheme: () => void
}

const Header: FC<HeaderProps> = ({ hasDevices, deviceCount, theme, toggleTheme }) => {
  return (
    <header
      style={{
        gridArea: 'header',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 20px',
        background: 'var(--bg-panel)',
        borderBottom: '1px solid var(--border)',
        boxShadow: '0 1px 0 var(--border)',
        zIndex: 100,
      }}
    >
      {/* Logo + Wordmark */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
        <div
          style={{
            width: 32,
            height: 32,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
            borderRadius: 'var(--r-md)',
            overflow: 'hidden',
          }}
        >
          <img src="/logo.png" alt="FlashTransfer Logo" style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
        </div>

        <div>
          <div
            style={{
              fontWeight: 700,
              fontSize: '15px',
              letterSpacing: '-0.3px',
              color: 'var(--text-primary)',
              lineHeight: 1,
            }}
          >
            FlashTransfer
          </div>
          <div
            style={{
              fontSize: '11px',
              color: 'var(--text-muted)',
              letterSpacing: '0.02em',
              lineHeight: 1,
              marginTop: '3px',
            }}
          >
            Local Network Transfer
          </div>
        </div>
      </div>

      {/* Right side: status badges */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
        {/* Device count badge */}
        {deviceCount > 0 && (
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '6px',
              padding: '4px 10px',
              borderRadius: 'var(--r-sm)',
              background: 'var(--bg-surface)',
              border: '1px solid var(--border)',
              fontSize: '12px',
              color: 'var(--text-secondary)',
            }}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <rect x="2" y="3" width="20" height="14" rx="2" stroke="currentColor" strokeWidth="2" />
              <path d="M8 21h8M12 17v4" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
            </svg>
            {deviceCount} device{deviceCount !== 1 ? 's' : ''} nearby
          </div>
        )}

        {/* Connection status */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '7px',
            padding: '5px 12px',
            borderRadius: 'var(--r-sm)',
            background: hasDevices ? 'var(--success-dim)' : 'var(--accent-dim)',
            border: `1px solid ${hasDevices ? 'rgba(34,211,165,0.3)' : 'var(--accent-border)'}`,
            fontSize: '12px',
            fontWeight: 500,
            color: hasDevices ? 'var(--success)' : 'var(--accent)',
            transition: 'all var(--t-base)',
          }}
          role="status"
          aria-label={hasDevices ? 'Devices connected' : 'Scanning for devices'}
        >
          {/* Animated dot */}
          <span
            style={{
              width: 7,
              height: 7,
              borderRadius: '50%',
              background: hasDevices ? 'var(--success)' : 'var(--accent)',
              animation: 'pulse-ring 2s ease-in-out infinite',
              display: 'inline-block',
              flexShrink: 0,
            }}
          />
          {hasDevices ? 'Connected' : 'Scanning…'}
        </div>

        {/* Theme Toggle Button */}
        <button
          onClick={toggleTheme}
          style={{
            background: 'none',
            border: 'none',
            color: 'var(--text-muted)',
            cursor: 'pointer',
            padding: '6px',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            borderRadius: '50%',
            transition: 'background var(--t-fast), color var(--t-fast)',
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.background = 'var(--bg-hover)'
            e.currentTarget.style.color = 'var(--text-primary)'
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.background = 'none'
            e.currentTarget.style.color = 'var(--text-muted)'
          }}
          aria-label="Toggle theme"
        >
          {theme === 'dark' ? (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          ) : (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <circle cx="12" cy="12" r="4" stroke="currentColor" strokeWidth="2" />
              <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          )}
        </button>
      </div>
    </header>
  )
}

export default Header
