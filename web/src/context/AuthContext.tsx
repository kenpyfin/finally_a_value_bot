import React, { createContext, useCallback, useContext, useEffect, useState } from 'react'
import { Button, Dialog, Flex, Text, TextField } from '@radix-ui/themes'
import { AUTH_REQUIRED_EVENT, sanitizeHttpHeaderValue, WEB_AUTH_STORAGE_KEY } from '../api/client'

type AuthContextValue = {
  authRequired: boolean
  authTokenInput: string
  setAuthTokenInput: (value: string) => void
  submitAuthToken: () => void
  setAuthRequired: (value: boolean) => void
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({
  children,
  onAuthError,
}: {
  children: React.ReactNode
  onAuthError: (message: string) => void
}) {
  const [authRequired, setAuthRequired] = useState(false)
  const [authTokenInput, setAuthTokenInput] = useState('')

  useEffect(() => {
    const onAuthRequired = () => setAuthRequired(true)
    window.addEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
    return () => window.removeEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
  }, [])

  const submitAuthToken = useCallback(() => {
    const token = sanitizeHttpHeaderValue(authTokenInput)
    if (!token) return
    if (token.length !== authTokenInput.trim().length) {
      onAuthError('Invalid API token: unsupported header characters.')
      return
    }
    sessionStorage.setItem(WEB_AUTH_STORAGE_KEY, token)
    setAuthRequired(false)
    setAuthTokenInput('')
    window.location.reload()
  }, [authTokenInput, onAuthError])

  const value: AuthContextValue = {
    authRequired,
    authTokenInput,
    setAuthTokenInput,
    submitAuthToken,
    setAuthRequired,
  }

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within AuthProvider')
  return ctx
}

export function AuthDialog() {
  const { authRequired, authTokenInput, setAuthTokenInput, submitAuthToken, setAuthRequired } =
    useAuth()

  return (
    <Dialog.Root open={authRequired} onOpenChange={(open) => !open && setAuthRequired(false)}>
      <Dialog.Content>
        <Dialog.Title>API token required</Dialog.Title>
        <Dialog.Description size="2" mb="3">
          This server requires an API token. Use the same value as <code>WEB_AUTH_TOKEN</code> in
          your .env.
        </Dialog.Description>
        <Flex direction="column" gap="3">
          <TextField.Root
            type="password"
            placeholder="Paste API token"
            value={authTokenInput}
            onChange={(e) => setAuthTokenInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submitAuthToken()
            }}
          />
          <Flex gap="2" justify="end">
            <Dialog.Close>
              <Button variant="soft" color="gray">
                Cancel
              </Button>
            </Dialog.Close>
            <Button onClick={submitAuthToken}>Save token</Button>
          </Flex>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  )
}
