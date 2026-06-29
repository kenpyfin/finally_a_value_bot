/** Visible snippet length in the composer quote chip (full text is still sent to the agent). */
export const REPLY_SNIPPET_MAX = 200

export type PendingReplyQuote = {
  messageId: string
  snippet: string
  fullContent: string
  senderName: string
  isFromBot: boolean
}

export function makeReplySnippet(text: string, max = REPLY_SNIPPET_MAX): string {
  const oneLine = text.replace(/\s+/g, ' ').trim()
  if (!oneLine) return ''
  if (oneLine.length <= max) return oneLine
  return `${oneLine.slice(0, max - 3)}...`
}

function escapeQuotedAttr(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
}

/** Compose the user message body sent to `/api/send_stream` (full quote + optional follow-up). */
export function formatReplyForSend(quote: PendingReplyQuote, userText: string): string {
  const role = quote.isFromBot ? 'assistant' : 'user'
  const sender = quote.senderName.trim() || role
  let block = `[quoted_message id="${escapeQuotedAttr(quote.messageId)}" role="${role}" sender="${escapeQuotedAttr(sender)}"]\n`
  block += quote.fullContent.trim()
  block += '\n[/quoted_message]'
  const trimmed = userText.trim()
  if (trimmed) {
    return `${block}\n\n${trimmed}`
  }
  return block
}
