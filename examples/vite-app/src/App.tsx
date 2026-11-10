/* eslint-disable react-refresh/only-export-components */
import { useState } from 'react'
import {
  createHashRouter,
  createMemoryRouter,
  Navigate,
  Outlet,
  RouterProvider,
  useLocation,
  type RouteObject,
} from 'react-router-dom'
import { AuthScreen } from './screens/AuthScreen'
import { NotFound } from './screens/NotFound'
import { AccountScreen, HomeScreen, WorkspaceShell } from './screens/Workspace'
import { destinationFrom, loginRedirect, requestedFrom } from './session'
import { SessionProvider, useSession } from './session-context'

function AppFrame() {
  const { signedIn } = useSession()
  const location = useLocation()
  if (location.pathname === '/') {
    return <Navigate to="/home" replace />
  }
  const isAuth = location.pathname === '/login' || location.pathname === '/forgot-password'
  const isProtected = location.pathname === '/home' || location.pathname === '/account'
  if (!signedIn && isProtected) {
    return <Navigate to={loginRedirect(requestedFrom(location.pathname, location.search))} replace />
  }
  if (signedIn && isAuth) {
    return <Navigate to={destinationFrom(new URLSearchParams(location.search).get('from'))} replace />
  }
  return <Outlet />
}

export const orbitRoutes: RouteObject[] = [
  {
    element: <AppFrame />,
    children: [
      { path: '/', element: null },
      { path: '/login', element: <AuthScreen key="login" /> },
      { path: '/forgot-password', element: <AuthScreen key="forgot-password" resetPassword /> },
      {
        element: <WorkspaceShell />,
        children: [
          { path: '/home', element: <HomeScreen /> },
          { path: '/account', element: <AccountScreen /> },
        ],
      },
      { path: '*', element: <NotFound /> },
    ],
  },
]

export function OrbitApp({
  router,
}: {
  router: ReturnType<typeof createHashRouter> | ReturnType<typeof createMemoryRouter>
}) {
  return (
    <SessionProvider>
      <RouterProvider router={router} />
    </SessionProvider>
  )
}

export function createOrbitMemoryRouter(initialEntries: string[] = ['/']) {
  return createMemoryRouter(orbitRoutes, { initialEntries })
}

export default function App() {
  const [router] = useState(() => createHashRouter(orbitRoutes))
  return <OrbitApp router={router} />
}
