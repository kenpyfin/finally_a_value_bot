import { describe, expect, it } from 'vitest'
import { formatUploadBytes, splitDataUrl } from './attachments'

describe('formatUploadBytes', () => {
  it('formats byte sizes', () => {
    expect(formatUploadBytes(512)).toBe('512 B')
    expect(formatUploadBytes(2048)).toBe('2.0 KB')
    expect(formatUploadBytes(10 * 1024 * 1024)).toBe('10.0 MB')
  })
})

describe('splitDataUrl', () => {
  it('parses data URLs', () => {
    const parsed = splitDataUrl('data:image/png;base64,abc123')
    expect(parsed?.mimeType).toBe('image/png')
    expect(parsed?.base64).toBe('abc123')
  })
})
