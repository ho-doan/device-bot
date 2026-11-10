import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Link, NavLink, Outlet, useNavigate } from 'react-router-dom'
import { OrbitLogo, SurfaceCard } from '../components'
import { DEMO_EMAIL, TASKS } from '../session'
import { useSession } from '../session-context'

function useStackStats() {
  const ref = useRef<HTMLDivElement>(null)
  const [stack, setStack] = useState(false)

  useEffect(() => {
    const node = ref.current
    if (!node) return undefined
    const update = () => {
      const root = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
      setStack(node.clientWidth < 640 || root > 24)
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(node)
    return () => observer.disconnect()
  }, [])

  return { ref, stack }
}

export function WorkspaceShell() {
  const { signOut } = useSession()
  const navigate = useNavigate()

  return (
    <div className="shell">
      <header className="topbar">
        <OrbitLogo />
        <button
          type="button"
          className="icon-btn"
          id="account-avatar-button"
          data-testid="account-avatar-control"
          title="Your account"
          aria-label="Your account"
          onClick={() => navigate('/account')}
        >
          <span className="avatar">AL</span>
        </button>
      </header>
      <div className="shell-body">
        <nav className="rail" aria-label="Workspace">
          <NavLink className={({ isActive }) => (isActive ? 'nav-item active' : 'nav-item')} to="/home">
            Overview
          </NavLink>
          <NavLink
            className={({ isActive }) => (isActive ? 'nav-item active' : 'nav-item')}
            to="/account"
          >
            Account
          </NavLink>
          <div className="rail-spacer" />
          <button
            type="button"
            className="icon-btn"
            id="rail-sign-out-button"
            data-testid="rail-sign-out-control"
            title="Sign out"
            aria-label="Sign out"
            onClick={signOut}
          >
            ⎋
          </button>
        </nav>
        <Outlet />
      </div>
      <nav className="bottom-nav" aria-label="Workspace">
        <NavLink className={({ isActive }) => (isActive ? 'nav-item active' : 'nav-item')} to="/home">
          Overview
        </NavLink>
        <NavLink className={({ isActive }) => (isActive ? 'nav-item active' : 'nav-item')} to="/account">
          Account
        </NavLink>
      </nav>
    </div>
  )
}

export function WorkspaceContent({ children }: { children: ReactNode }) {
  return (
    <main className="workspace" id="orbit-viewport" data-testid="orbit-viewport">
      <div className="workspace-inner">{children}</div>
    </main>
  )
}

export function HomeScreen() {
  const [completed, setCompleted] = useState<Set<number>>(new Set([0]))
  const { ref, stack } = useStackStats()

  function toggle(index: number, checked: boolean) {
    setCompleted((current) => {
      const next = new Set(current)
      if (checked) next.add(index)
      else next.delete(index)
      return next
    })
  }

  return (
    <WorkspaceContent>
      <div className="eyebrow">YOUR WORKSPACE</div>
      <h1 className="headline">A fresh start, Alex.</h1>
      <p className="lede">Let’s make a little progress on what matters.</p>
      <section className="hero-card">
        <h2>Find your flow.</h2>
        <p>{'Big ideas begin with small, intentional steps.\nYour next one is right here.'}</p>
        <Link
          id="view-profile-button"
          data-testid="view-profile-button"
          className="btn btn-filled"
          to="/account"
        >
          View your profile
        </Link>
      </section>
      <div className={stack ? 'stats stack' : 'stats'} ref={ref}>
        <SurfaceCard>
          <div>{`${completed.size}/3`}</div>
          <div>Small wins</div>
        </SurfaceCard>
        <SurfaceCard>
          <div>20 min</div>
          <div>Time for yourself</div>
        </SurfaceCard>
        <SurfaceCard>
          <div>Today</div>
          <div>A good day to begin</div>
        </SurfaceCard>
      </div>
      <SurfaceCard>
        <h2>A little plan for today</h2>
        <p>Check things off. Enjoy the small wins.</p>
        {TASKS.map((task, index) => {
          const done = completed.has(index)
          return (
            <label
              key={task.title}
              className={done ? 'task done' : 'task'}
              id={`task-${index + 1}-checkbox`}
            >
              <input
                type="checkbox"
                data-testid={`task-${index + 1}-checkbox`}
                checked={done}
                onChange={(event) => toggle(index, event.target.checked)}
              />
              <span className="task-copy">
                <span className="task-title">{task.title}</span>
                <small>{task.detail}</small>
              </span>
            </label>
          )
        })}
        <div
          className="progress theme"
          role="progressbar"
          aria-label={`${completed.size} of 3 tasks complete`}
          aria-valuenow={completed.size}
          aria-valuemin={0}
          aria-valuemax={3}
        >
          <span style={{ width: `${(completed.size / 3) * 100}%` }} />
        </div>
      </SurfaceCard>
      <p className="tagline">Your pace. Your space. Your orbit.</p>
    </WorkspaceContent>
  )
}

export function AccountScreen() {
  const { signOut } = useSession()
  return (
    <WorkspaceContent>
      <h1 className="headline">Your account</h1>
      <p className="lede">A familiar face in your own little space.</p>
      <SurfaceCard>
        <div className="avatar lg">AL</div>
        <h2>Alex Lane</h2>
        <p className="credentials">{DEMO_EMAIL}</p>
        <hr />
        <h3>Your personal workspace</h3>
        <p>
          You’re exploring a demo account. Changes and your sign-in session are kept only
          while the app is running.
        </p>
        <button
          type="button"
          className="btn btn-outlined"
          id="sign-out-button"
          data-testid="sign-out-button"
          onClick={signOut}
        >
          Sign out
        </button>
      </SurfaceCard>
    </WorkspaceContent>
  )
}
