import React from 'react'
import { Button, Text } from '@radix-ui/themes'

export type EmptyStateProps = {
  title: string
  description?: string
  actionLabel?: string
  onAction?: () => void
  className?: string
}

export function EmptyState({
  title,
  description,
  actionLabel,
  onAction,
  className,
}: EmptyStateProps) {
  return (
    <div className={`mc-empty-state ${className ?? ''}`.trim()}>
      <Text size="2" weight="medium" className="block text-[color:var(--mc-text-primary)]">
        {title}
      </Text>
      {description ? (
        <Text size="1" color="gray" className="mt-1 block">
          {description}
        </Text>
      ) : null}
      {actionLabel && onAction ? (
        <Button size="1" variant="soft" className="mt-3 cursor-pointer" onClick={onAction}>
          {actionLabel}
        </Button>
      ) : null}
    </div>
  )
}
