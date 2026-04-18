const WEB_AUTH_STORAGE_KEY = 'web_auth_token'

export { WEB_AUTH_STORAGE_KEY }

export const AUTH_REQUIRED_EVENT = 'web-auth-required'

function sanitizeHttpHeaderValue(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  if (trimmed.includes('\r') || trimmed.includes('\n')) return null
  for (let i = 0; i < trimmed.length; i += 1) {
    const code = trimmed.charCodeAt(i)
    if (code > 0xff) return null
  }
  return trimmed
}

export function getStoredAuthToken(): string | null {
  if (typeof sessionStorage === 'undefined') return null
  try {
    const t = sessionStorage.getItem(WEB_AUTH_STORAGE_KEY)
    if (!t) return null
    const sanitized = sanitizeHttpHeaderValue(t)
    if (!sanitized) {
      sessionStorage.removeItem(WEB_AUTH_STORAGE_KEY)
      return null
    }
    return sanitized
  } catch {
    return null
  }
}

export function makeHeaders(options: RequestInit = {}): HeadersInit {
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string> | undefined),
  }
  for (const [key, value] of Object.entries(headers)) {
    if (typeof value !== 'string') {
      delete headers[key]
      continue
    }
    const sanitized = sanitizeHttpHeaderValue(value)
    if (!sanitized) {
      delete headers[key]
      continue
    }
    headers[key] = sanitized
  }
  const token = getStoredAuthToken()
  if (token) {
    headers['Authorization'] = `Bearer ${token}`
  }
  if (options.body && !headers['Content-Type']) {
    headers['Content-Type'] = 'application/json'
  }
  return headers
}

function messageForFailedResponse(status: number, data: Record<string, unknown>, bodyText?: string): string {
  if (status === 401) {
    return 'Unauthorized. Enter the API token (WEB_AUTH_TOKEN from .env).'
  }
  if (status === 429) {
    const serverMsg = String(data.error || data.message || bodyText || '').trim()
    return serverMsg
      ? `Too many requests: ${serverMsg} Please wait a moment before sending again.`
      : 'Too many requests. Please wait a moment before sending again.'
  }
  return String(data.error || data.message || bodyText || `HTTP ${status}`)
}

export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const res = await fetch(path, { ...options, headers: makeHeaders(options) })
  const bodyText = await res.text()
  let data: Record<string, unknown> = {}
  try {
    data = bodyText ? (JSON.parse(bodyText) as Record<string, unknown>) : {}
  } catch {
    data = { message: bodyText || undefined }
  }
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent(AUTH_REQUIRED_EVENT))
    throw new Error(messageForFailedResponse(401, data, bodyText))
  }
  if (!res.ok) {
    throw new Error(messageForFailedResponse(res.status, data, bodyText))
  }
  return data as T
}

export { sanitizeHttpHeaderValue }
