import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { createOrbitMemoryRouter, OrbitApp } from './App'
import { AUTH_DELAY_MS, DEMO_EMAIL } from './session'

function renderApp(location = '/') {
  const router = createOrbitMemoryRouter([location])
  render(<OrbitApp router={router} />)
  return router
}

async function demoLogin(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId('demo-button'))
  await waitFor(
    () => {
      expect(
        screen.queryByText('A fresh start, Alex.') ?? screen.queryByText('Alex Lane'),
      ).toBeTruthy()
    },
    { timeout: AUTH_DELAY_MS + 1500 },
  )
}

describe('Orbit', () => {
  it('validates fields, toggles password, rejects bad credentials', async () => {
    const user = userEvent.setup()
    renderApp()
    await screen.findByTestId('email-field')
    await user.click(screen.getByTestId('submit-button'))
    expect(screen.getByText('Enter a valid email address.')).toBeInTheDocument()
    expect(screen.getByText('Enter your password.')).toBeInTheDocument()
    await user.type(screen.getByTestId('email-field'), DEMO_EMAIL)
    await user.type(screen.getByTestId('password-field'), 'incorrect')
    await user.click(screen.getByTitle('Show password'))
    expect(screen.getByTestId('password-field')).toHaveValue('incorrect')
    expect(screen.getByTitle('Hide password')).toBeInTheDocument()
    await user.click(screen.getByTestId('submit-button'))
    expect(screen.getByTestId('submit-button')).toBeDisabled()
    expect(
      await screen.findByText(
        'Email or password is incorrect. Try the demo account below.',
        {},
        { timeout: AUTH_DELAY_MS + 1000 },
      ),
    ).toBeInTheDocument()
    expect(screen.getByTestId('email-field')).toBeInTheDocument()
  })

  it('demo login, tasks, profile, logout and protected route', async () => {
    const user = userEvent.setup()
    const router = renderApp()
    await screen.findByTestId('email-field')
    await demoLogin(user)
    await user.click(screen.getByText('Choose your most important task'))
    expect(screen.getByText('2/3')).toBeInTheDocument()
    await user.click(screen.getByText('View your profile'))
    expect(screen.getByText('Alex Lane')).toBeInTheDocument()
    await user.click(screen.getByTestId('sign-out-button'))
    expect(screen.getByText('Welcome back.')).toBeInTheDocument()
    expect(screen.queryByText('Alex Lane')).not.toBeInTheDocument()
    await act(async () => {
      await router.navigate('/home')
    })
    expect(screen.getByText('Welcome back.')).toBeInTheDocument()
    expect(screen.queryByText('A fresh start, Alex.')).not.toBeInTheDocument()
  })

  it('protected deep link returns to intended account after sign in', async () => {
    const user = userEvent.setup()
    renderApp('/account')
    expect(await screen.findByText('Welcome back.')).toBeInTheDocument()
    expect(screen.queryByText('Alex Lane')).not.toBeInTheDocument()
    await demoLogin(user)
    expect(screen.getByText('Alex Lane')).toBeInTheDocument()
    await user.click(screen.getByTestId('sign-out-button'))
    await demoLogin(user)
    expect(screen.getByText('Alex Lane')).toBeInTheDocument()
  })

  it('reset validates, simulates request, and preserves return route', async () => {
    const user = userEvent.setup()
    renderApp('/account')
    await screen.findByText('Forgot password?')
    await user.click(screen.getByText('Forgot password?'))
    expect(screen.getByText('Forgot your password?')).toBeInTheDocument()
    await user.click(screen.getByTestId('submit-button'))
    expect(screen.getByText('Enter a valid email address.')).toBeInTheDocument()
    await user.type(screen.getByTestId('email-field'), DEMO_EMAIL)
    await user.click(screen.getByTestId('submit-button'))
    expect(
      await screen.findByText('You’re all set.', {}, { timeout: AUTH_DELAY_MS + 1000 }),
    ).toBeInTheDocument()
    await user.click(screen.getByText('Back to sign in'))
    await screen.findByTestId('demo-button')
    await demoLogin(user)
    expect(screen.getByText('Alex Lane')).toBeInTheDocument()
  })

  it('unknown routes provide a usable way home', async () => {
    const user = userEvent.setup()
    renderApp('/missing')
    expect(await screen.findByText('This page is outside your orbit.')).toBeInTheDocument()
    await user.click(screen.getByText('Back to home'))
    expect(await screen.findByText('Welcome back.')).toBeInTheDocument()
  })

  it('only restores account when from is exactly /account', async () => {
    const user = userEvent.setup()
    renderApp('/account?tab=profile')
    await screen.findByTestId('email-field')
    await demoLogin(user)
    expect(screen.getByText('A fresh start, Alex.')).toBeInTheDocument()
  })
})
