import { useRef, useState, type FormEvent } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router-dom'
import { OrbitLogo, SurfaceCard, WelcomePanel } from '../components'
import {
  DEMO_EMAIL,
  DEMO_PASSWORD,
  isValidEmail,
  simulateReset,
} from '../session'
import { useSession } from '../session-context'

export function AuthScreen({ resetPassword = false }: { resetPassword?: boolean }) {
  const { signIn } = useSession()
  const navigate = useNavigate()
  const [params] = useSearchParams()
  const returnTo = params.get('from')
  const errorRef = useRef<HTMLDivElement>(null)
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [obscure, setObscure] = useState(true)
  const [busy, setBusy] = useState(false)
  const [sent, setSent] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [emailError, setEmailError] = useState<string | null>(null)
  const [passwordError, setPasswordError] = useState<string | null>(null)

  const fromQuery = returnTo ? `?from=${encodeURIComponent(returnTo)}` : ''

  async function submit(nextEmail = email, nextPassword = password) {
    if (busy) return
    const nextEmailError = isValidEmail(nextEmail) ? null : 'Enter a valid email address.'
    const nextPasswordError = resetPassword
      ? null
      : nextPassword.length === 0
        ? 'Enter your password.'
        : null
    if (nextEmailError || nextPasswordError) {
      setEmailError(nextEmailError)
      setPasswordError(nextPasswordError)
      setError('Check the highlighted fields to continue.')
      queueMicrotask(() => errorRef.current?.focus())
      return
    }
    setBusy(true)
    setError(null)
    setEmailError(null)
    setPasswordError(null)
    if (resetPassword) {
      await simulateReset()
      setBusy(false)
      setSent(true)
      return
    }
    const success = await signIn(nextEmail, nextPassword)
    if (success) return
    setBusy(false)
    setError('Email or password is incorrect. Try the demo account below.')
    queueMicrotask(() => errorRef.current?.focus())
  }

  function onSubmit(event: FormEvent) {
    event.preventDefault()
    void submit()
  }

  return (
    <div className="auth-layout">
      <WelcomePanel />
      <div className="auth-form-pane" id="orbit-viewport" data-testid="orbit-viewport">
        <div className="auth-form">
          <OrbitLogo />
          {sent ? (
            <div>
              <h1 className="headline">You’re all set.</h1>
              <p className="lede">
                Reset request simulated. This demo does not send email or change your
                password.
              </p>
              <button
                type="button"
                id="back-to-sign-in-button"
                data-testid="back-to-sign-in-button"
                className="btn btn-filled"
                onClick={() => navigate(`/login${fromQuery}`)}
              >
                Back to sign in
              </button>
            </div>
          ) : (
            <form onSubmit={onSubmit} autoComplete="on">
              <div className="eyebrow">
                {resetPassword ? 'LET’S GET YOU BACK' : 'YOUR SPACE TO FOCUS'}
              </div>
              <h1 className="headline">
                {resetPassword ? 'Forgot your password?' : 'Welcome back.'}
              </h1>
              <p className="lede">
                {resetPassword
                  ? 'Enter your email to try the password reset flow.'
                  : 'Make room for your best work. Sign in to your workspace.'}
              </p>
              {error ? (
                <div
                  className="banner"
                  role="alert"
                  tabIndex={-1}
                  ref={errorRef}
                  aria-live="polite"
                >
                  {error}
                </div>
              ) : null}
              <div className={emailError ? 'field error' : 'field'}>
                <label htmlFor="email-field">Email address</label>
                <div className="field-control">
                  <input
                    id="email-field"
                    data-testid="email-field"
                    name="email"
                    type="email"
                    inputMode="email"
                    autoComplete="username email"
                    autoCorrect="off"
                    placeholder="you@example.com"
                    value={email}
                    disabled={busy}
                    onChange={(event) => setEmail(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' && resetPassword) {
                        event.preventDefault()
                        void submit()
                      }
                    }}
                  />
                </div>
                {emailError ? <div className="field-error">{emailError}</div> : null}
              </div>
              {resetPassword ? null : (
                <>
                  <div className={passwordError ? 'field error' : 'field'}>
                    <label htmlFor="password-field">Password</label>
                    <div className="field-control">
                      <input
                        id="password-field"
                        data-testid="password-field"
                        name="password"
                        type={obscure ? 'password' : 'text'}
                        autoComplete="current-password"
                        autoCorrect="off"
                        value={password}
                        disabled={busy}
                        onChange={(event) => setPassword(event.target.value)}
                      />
                      <button
                        type="button"
                        className="icon-btn"
                        id="password-toggle-button"
                        data-testid="password-visibility-control"
                        title={obscure ? 'Show password' : 'Hide password'}
                        aria-label={obscure ? 'Show password' : 'Hide password'}
                        onClick={() => setObscure((value) => !value)}
                      >
                        {obscure ? 'Show' : 'Hide'}
                      </button>
                    </div>
                    {passwordError ? <div className="field-error">{passwordError}</div> : null}
                  </div>
                  <div className="row-end">
                    <Link
                      id="forgot-password-button"
                      data-testid="forgot-password-button"
                      className={`btn-text ${busy ? 'btn-text' : ''}`}
                      to={busy ? '#' : `/forgot-password${fromQuery}`}
                      onClick={(event) => {
                        if (busy) event.preventDefault()
                      }}
                      aria-disabled={busy}
                    >
                      Forgot password?
                    </Link>
                  </div>
                </>
              )}
              <button
                type="submit"
                className="btn btn-filled block"
                id={resetPassword ? 'password-reset-button' : 'sign-in-button'}
                data-testid="submit-button"
                disabled={busy}
              >
                {busy ? (
                  <span className="spinner" role="status" aria-label="Please wait" />
                ) : (
                  <>
                    {resetPassword ? 'Try password reset' : 'Sign in'}
                    <span aria-hidden="true">→</span>
                  </>
                )}
              </button>
              {resetPassword ? (
                <button
                  type="button"
                  className="btn-text"
                  id="back-to-sign-in-button"
                  data-testid="back-to-sign-in-button"
                  disabled={busy}
                  onClick={() => navigate(`/login${fromQuery}`)}
                >
                  ← Back to sign in
                </button>
              ) : (
                <>
                  <div style={{ height: 24 }} />
                  <SurfaceCard compact>
                    <strong>Just exploring?</strong>
                    <p>A ready-to-use workspace. No account needed.</p>
                    <p className="credentials">{`${DEMO_EMAIL}\n${DEMO_PASSWORD}`}</p>
                    <button
                      type="button"
                      className="btn btn-outlined"
                      id="demo-button"
                      data-testid="demo-button"
                      disabled={busy}
                      onClick={() => {
                        setEmail(DEMO_EMAIL)
                        setPassword(DEMO_PASSWORD)
                        void submit(DEMO_EMAIL, DEMO_PASSWORD)
                      }}
                    >
                      Try demo workspace
                    </button>
                  </SurfaceCard>
                  <p className="fine-print">
                    Demo mode · Your session ends when the app restarts.
                  </p>
                </>
              )}
            </form>
          )}
          <p className="tagline">A little less noise. A lot more focus.</p>
        </div>
      </div>
    </div>
  )
}
