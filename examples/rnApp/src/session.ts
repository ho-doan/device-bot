export const DEMO_EMAIL = 'alex@orbit.demo';
export const DEMO_PASSWORD = 'Orbit123!';
export const AUTH_DELAY_MS = 700;
export const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function isValidEmail(value: string): boolean {
  return EMAIL_PATTERN.test(value.trim());
}

export function credentialsMatch(email: string, password: string): boolean {
  return email.trim().toLowerCase() === DEMO_EMAIL && password === DEMO_PASSWORD;
}

export function destinationFrom(
  from: string | null | undefined,
): '/account' | '/home' {
  return from === '/account' ? '/account' : '/home';
}

export const TASKS = [
  {title: 'Make space for a fresh start', detail: '5 min · A little planning'},
  {
    title: 'Choose your most important task',
    detail: '10 min · Set your intention',
  },
  {title: 'Take a mindful break', detail: '5 min · Recharge'},
] as const;

export const KNOWN_PATHS = new Set([
  '/',
  '/login',
  '/forgot-password',
  '/home',
  '/account',
]);

export type OrbitLocation = {path: string; from: string | null};

export function resolveLocation(
  path: string,
  from: string | null,
  signedIn: boolean,
): OrbitLocation {
  if (path === '/') {
    return resolveLocation('/home', from, signedIn);
  }
  const isAuth = path === '/login' || path === '/forgot-password';
  const isProtected = path === '/home' || path === '/account';
  if (!signedIn && isProtected) {
    return {path: '/login', from: path};
  }
  if (signedIn && isAuth) {
    return {path: destinationFrom(from), from: null};
  }
  return {path, from};
}

export function delay(ms: number): Promise<void> {
  return new Promise(resolve => {
    setTimeout(resolve, ms);
  });
}

export async function authenticate(
  email: string,
  password: string,
): Promise<boolean> {
  await delay(AUTH_DELAY_MS);
  return credentialsMatch(email, password);
}

export async function simulateReset(): Promise<void> {
  await delay(AUTH_DELAY_MS);
}
