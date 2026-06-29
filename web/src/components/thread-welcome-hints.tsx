import React from 'react'

/** Empty thread welcome with keyboard shortcut hints. */
export function ThreadWelcomeHints({
  onShowShortcuts,
}: {
  onShowShortcuts?: () => void
}) {
  return (
    <div className="mc-thread-welcome-hints px-3 py-8 text-center">
      <p className="text-sm text-[color:var(--mc-text-muted)]">
        Send a message to start. Attach files by dragging them into the composer.
      </p>
      <p className="mt-2 text-xs text-[color:var(--mc-text-faint)]">
        Press <kbd className="mc-shortcuts-kbd">/</kbd> to focus the composer
        {onShowShortcuts ? (
          <>
            {' '}
            · Press{' '}
            <button
              type="button"
              className="mc-thread-welcome-shortcuts-link cursor-pointer"
              onClick={onShowShortcuts}
            >
              <kbd className="mc-shortcuts-kbd">?</kbd>
            </button>{' '}
            for shortcuts
          </>
        ) : (
          <>
            {' '}
            · Press <kbd className="mc-shortcuts-kbd">?</kbd> for shortcuts
          </>
        )}
      </p>
    </div>
  )
}
