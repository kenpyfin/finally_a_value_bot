import React from 'react'
import { ThreadPrimitive } from '@assistant-ui/react'
import { IconChevronDown } from './icons'

/** Jump to the latest messages when the thread is scrolled up. */
export function ScrollToLatest() {
  return (
    <ThreadPrimitive.ScrollToBottom asChild>
      <button
        type="button"
        className="mc-scroll-to-latest-btn cursor-pointer"
        aria-label="Jump to latest messages"
        title="Jump to latest messages"
      >
        <IconChevronDown className="size-4 shrink-0" />
      </button>
    </ThreadPrimitive.ScrollToBottom>
  )
}
