import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  existingNames: string[]
  onCreate: (name: string) => Promise<void>
}

export function NewPersonaDialog({ open, onOpenChange, existingNames, onCreate }: Props) {
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (open) {
      setName('')
      setError('')
    }
  }, [open])

  const trimmed = name.trim()
  const duplicate = existingNames.some((n) => n === trimmed)
  const reserved = trimmed === 'default'
  const validationError = duplicate
    ? `A persona named “${trimmed}” already exists.`
    : reserved
      ? '“default” is reserved.'
      : ''
  const canCreate = trimmed.length > 0 && !validationError && !busy

  const submit = async () => {
    if (!canCreate) return
    setBusy(true)
    setError('')
    try {
      await onCreate(trimmed)
      onOpenChange(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="max-w-[420px]">
        <DialogHeader>
          <DialogTitle>New persona</DialogTitle>
          <DialogDescription>
            Each persona has its own chat history, memory, and engine settings.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <Input
            placeholder="e.g. research, sourdough, ops"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            disabled={busy}
            aria-label="Persona name"
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                void submit()
              }
            }}
          />
          {validationError || error ? (
            <p className="text-xs text-destructive">{validationError || error}</p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => void submit()} disabled={!canCreate}>
            {busy ? 'Creating…' : 'Create persona'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
