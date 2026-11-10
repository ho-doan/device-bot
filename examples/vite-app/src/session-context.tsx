/* eslint-disable react-refresh/only-export-components */
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'
import { authenticate } from './session'

type SessionValue = {
  signedIn: boolean
  signIn: (email: string, password: string) => Promise<boolean>
  signOut: () => void
}

const SessionContext = createContext<SessionValue | null>(null)

export function SessionProvider({ children }: { children: ReactNode }) {
  const [signedIn, setSignedIn] = useState(false)

  const signIn = useCallback(async (email: string, password: string) => {
    const success = await authenticate(email, password)
    if (success) setSignedIn(true)
    return success
  }, [])

  const signOut = useCallback(() => setSignedIn(false), [])

  const value = useMemo(
    () => ({ signedIn, signIn, signOut }),
    [signedIn, signIn, signOut],
  )

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>
}

export function useSession(): SessionValue {
  const value = useContext(SessionContext)
  if (!value) throw new Error('SessionProvider is required')
  return value
}
