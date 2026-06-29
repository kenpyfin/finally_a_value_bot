import React from 'react'
import { Callout } from '@radix-ui/themes'

export type ErrorBannerProps = {
  message: string
  className?: string
  onDismiss?: () => void
}

export function ErrorBanner({ message, className, onDismiss }: ErrorBannerProps) {
  if (!message.trim()) return null
  return (
    <div role="alert" aria-live="assertive" className={className}>
      <Callout.Root color="red" size="1" variant="soft">
        <Callout.Text>
          {message}
          {onDismiss ? (
            <button
              type="button"
              className="mc-error-dismiss"
              onClick={onDismiss}
              aria-label="Dismiss error"
            >
              Dismiss
            </button>
          ) : null}
        </Callout.Text>
      </Callout.Root>
    </div>
  )
}
