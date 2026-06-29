import { describe, expect, it } from 'vitest'
import { formatReplyForSend, makeReplySnippet } from './reply-quote'

describe('makeReplySnippet', () => {
  it('collapses whitespace and truncates long text', () => {
    const long = 'a'.repeat(250)
    expect(makeReplySnippet(`line one\n\nline two`)).toBe('line one line two')
    expect(makeReplySnippet(long)).toBe(`${'a'.repeat(197)}...`)
  })
})

describe('formatReplyForSend', () => {
  it('wraps full content and appends user follow-up', () => {
    const out = formatReplyForSend(
      {
        messageId: 'msg-1',
        snippet: 'hi',
        fullContent: 'full body\nwith lines',
        senderName: 'bot',
        isFromBot: true,
      },
      'follow up',
    )
    expect(out).toContain('[quoted_message id="msg-1" role="assistant" sender="bot"]')
    expect(out).toContain('full body\nwith lines')
    expect(out).toContain('[/quoted_message]')
    expect(out.endsWith('follow up')).toBe(true)
  })
})
