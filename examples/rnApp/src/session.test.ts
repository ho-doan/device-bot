import {
  credentialsMatch,
  DEMO_EMAIL,
  DEMO_PASSWORD,
  destinationFrom,
  isValidEmail,
  resolveLocation,
} from './session';

describe('demo session', () => {
  it('accepts trimmed lowercase email and exact password', () => {
    expect(isValidEmail('  alex@orbit.demo ')).toBe(true);
    expect(credentialsMatch('  Alex@Orbit.demo ', DEMO_PASSWORD)).toBe(true);
    expect(credentialsMatch(DEMO_EMAIL, ' Orbit123! ')).toBe(false);
  });

  it('only restores /account from an exact from value', () => {
    expect(destinationFrom('/account')).toBe('/account');
    expect(destinationFrom('/account?tab=profile')).toBe('/home');
  });

  it('guards protected routes and unknown paths', () => {
    expect(resolveLocation('/home', null, false)).toEqual({
      path: '/login',
      from: '/home',
    });
    expect(resolveLocation('/account', null, false)).toEqual({
      path: '/login',
      from: '/account',
    });
    expect(resolveLocation('/login', '/account', true)).toEqual({
      path: '/account',
      from: null,
    });
    expect(resolveLocation('/missing', null, true).path).toBe('/missing');
  });
});
