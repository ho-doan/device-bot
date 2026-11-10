import type { ReactNode } from 'react'

export function OrbitLogo({ onDark = false }: { onDark?: boolean }) {
  return (
    <div className={onDark ? 'orbit-logo on-dark' : 'orbit-logo'}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="1.8" />
        <circle cx="12" cy="12" r="3.2" fill="currentColor" />
        <circle cx="18.5" cy="7" r="1.6" fill="currentColor" />
      </svg>
      <span>orbit</span>
    </div>
  )
}

export function SurfaceCard({
  children,
  compact = false,
}: {
  children: ReactNode
  compact?: boolean
}) {
  return <div className={compact ? 'surface-card compact' : 'surface-card'}>{children}</div>
}

export function WelcomePanel() {
  return (
    <aside className="welcome">
      <OrbitLogo onDark />
      <h2>
        Good work starts
        <br />
        with a clear mind.
      </h2>
      <p>{'Your plans, projects, and small wins.\nTogether in one calm space.'}</p>
      <div className="welcome-card">
        <div className="kicker">A LITTLE FOCUS GOES A LONG WAY</div>
        <h3>{'One thing at a time.\nOne step closer.'}</h3>
        <div className="progress" aria-hidden="true">
          <span style={{ width: '70%' }} />
        </div>
        <p className="welcome-caption">Progress, at your own pace.</p>
      </div>
      <div className="welcome-foot">LESS FRICTION. MORE FLOW.</div>
    </aside>
  )
}
