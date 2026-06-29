import React from 'react'
import { AlertDialog, Button, Flex } from '@radix-ui/themes'

export type ConfirmDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  confirmLabel?: string
  cancelLabel?: string
  destructive?: boolean
  loading?: boolean
  onConfirm: () => void | Promise<void>
}

export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  destructive = false,
  loading = false,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <AlertDialog.Root open={open} onOpenChange={onOpenChange}>
      <AlertDialog.Content maxWidth="420px">
        <AlertDialog.Title>{title}</AlertDialog.Title>
        <AlertDialog.Description size="2">{description}</AlertDialog.Description>
        <Flex gap="3" mt="4" justify="end">
          <AlertDialog.Cancel>
            <Button variant="soft" color="gray" disabled={loading}>
              {cancelLabel}
            </Button>
          </AlertDialog.Cancel>
          <AlertDialog.Action>
            <Button
              color={destructive ? 'red' : undefined}
              variant="solid"
              disabled={loading}
              onClick={(e) => {
                e.preventDefault()
                void onConfirm()
              }}
            >
              {loading ? 'Working…' : confirmLabel}
            </Button>
          </AlertDialog.Action>
        </Flex>
      </AlertDialog.Content>
    </AlertDialog.Root>
  )
}

export type PendingConfirm = {
  title: string
  description: string
  confirmLabel?: string
  destructive?: boolean
  onConfirm: () => void | Promise<void>
}

export function useConfirmDialog() {
  const [pending, setPending] = React.useState<PendingConfirm | null>(null)
  const [loading, setLoading] = React.useState(false)

  const requestConfirm = React.useCallback((opts: PendingConfirm) => {
    setPending(opts)
  }, [])

  const close = React.useCallback(() => {
    if (loading) return
    setPending(null)
  }, [loading])

  const handleConfirm = React.useCallback(async () => {
    if (!pending) return
    setLoading(true)
    try {
      await pending.onConfirm()
      setPending(null)
    } finally {
      setLoading(false)
    }
  }, [pending])

  const dialog = (
    <ConfirmDialog
      open={pending != null}
      onOpenChange={(open) => {
        if (!open) close()
      }}
      title={pending?.title ?? ''}
      description={pending?.description ?? ''}
      confirmLabel={pending?.confirmLabel}
      destructive={pending?.destructive}
      loading={loading}
      onConfirm={handleConfirm}
    />
  )

  return { requestConfirm, confirmDialog: dialog }
}
