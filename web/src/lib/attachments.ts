import { apiForm } from '../api/client'

export type SendAttachmentRef = {
  filename?: string
  media_type?: string
  tool_path: string
  url: string
}

export type UploadAttachmentResponse = {
  filename: string
  media_type: string
  bytes: number
  tool_path: string
  url: string
}

export function formatUploadBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return 'unknown size'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function splitDataUrl(value: string): { mimeType?: string; base64: string } | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  if (!trimmed.startsWith('data:')) return { base64: trimmed }
  const comma = trimmed.indexOf(',')
  if (comma < 0) return null
  const header = trimmed.slice(5, comma)
  const base64 = trimmed.slice(comma + 1)
  const mimeType = header.split(';')[0] || undefined
  return { mimeType, base64 }
}

export function dataUrlToFile(dataUrl: string, filename?: string): File {
  const parsed = splitDataUrl(dataUrl)
  if (!parsed?.base64) {
    throw new Error('invalid data URL')
  }
  const binary = atob(parsed.base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  const mime = parsed.mimeType || 'application/octet-stream'
  const name = filename || `upload.${mime.split('/')[1] || 'bin'}`
  return new File([bytes], name, { type: mime })
}

/** Chunked base64 for legacy inline fallback (prefer multipart upload). */
export async function fileToBase64(file: File): Promise<string> {
  const buf = await file.arrayBuffer()
  const bytes = new Uint8Array(buf)
  const chunkSize = 0x8000
  let binary = ''
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const slice = bytes.subarray(i, i + chunkSize)
    binary += String.fromCharCode(...slice)
  }
  return btoa(binary)
}

export async function uploadAttachmentFile(
  file: File,
  chatId: number | null,
  options?: { signal?: AbortSignal; onProgress?: (message: string) => void },
): Promise<SendAttachmentRef> {
  const form = new FormData()
  form.append('file', file, file.name || 'upload')
  const query = chatId != null ? `?chat_id=${encodeURIComponent(String(chatId))}` : ''
  options?.onProgress?.(`Uploading ${file.name || 'file'} (${formatUploadBytes(file.size)})…`)
  const data = await apiForm<UploadAttachmentResponse>(`/api/uploads${query}`, {
    method: 'POST',
    body: form,
    signal: options?.signal,
  })
  if (!data.tool_path || !data.url) {
    throw new Error('upload response missing tool_path or url')
  }
  return {
    filename: data.filename || file.name,
    media_type: data.media_type || file.type || undefined,
    tool_path: data.tool_path,
    url: data.url,
  }
}
