import { describe, expect, it } from 'vitest'
import {
  AUTH_DELAY_MS,
  authenticate,
  credentialsMatch,
  DEMO_EMAIL,
  DEMO_PASSWORD,
  destinationFrom,
  isValidEmail,
} from './session'

describe('demo session', () => {
  it('accepts trimmed lowercase email and exact password', () => {
    expect(isValidEmail('  alex@orbit.demo ')).toBe(true)
    expect(credentialsMatch('  Alex@Orbit.demo ', DEMO_PASSWORD)).toBe(true)
    expect(credentialsMatch(DEMO_EMAIL, ' Orbit123! ')).toBe(false)
    expect(credentialsMatch(DEMO_EMAIL, 'orbit123!')).toBe(false)
  })

  it('only restores /account from an exact from value', () => {
    expect(destinationFrom('/account')).toBe('/account')
    expect(destinationFrom('/account?tab=profile')).toBe('/home')
    expect(destinationFrom('/home')).toBe('/home')
    expect(destinationFrom(null)).toBe('/home')
  })

  it('authenticates after the demo delay', async () => {
    const started = Date.now()
    await expect(authenticate(DEMO_EMAIL, DEMO_PASSWORD)).resolves.toBe(true)
    expect(Date.now() - started).toBeGreaterThanOrEqual(AUTH_DELAY_MS - 20)
    await expect(authenticate(DEMO_EMAIL, 'nope')).resolves.toBe(false)
  })
})
