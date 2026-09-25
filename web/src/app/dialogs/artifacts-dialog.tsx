import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from '@/components/ui/select'
import { Dialog, DialogContent, DialogTitle, DialogDescription, DialogClose } from '@/components/ui/dialog'
import React from 'react'

import remarkGfm from 'remark-gfm'
import ReactMarkdown from 'react-markdown'
import { ArtifactListSkeleton, ContentPreviewSkeleton } from '../../components/skeleton'
import { artifactPreviewUrl, formatBytes } from './shared'

import type { ArtifactsDialogProps } from './types'

function MarkdownExternalLink(props: React.ComponentPropsWithoutRef<'a'>) {
  const mergedRel = [props.rel, 'noopener', 'noreferrer'].filter(Boolean).join(' ')
  return <a {...props} target="_blank" rel={mergedRel} />
}

export function ArtifactsDialog(props: ArtifactsDialogProps) {
const { appearance, chatId, activePersonaId, artifacts } = props
  const artifactsDialogOpen = artifacts.open
  const setArtifactsDialogOpen = artifacts.onOpenChange
  const setArtifactsError = artifacts.setError
  const setArtifactTextError = artifacts.setTextError
  const artifactKindFilter = artifacts.kindFilter
  const setArtifactKindFilter = artifacts.setKindFilter
  const loadArtifacts = artifacts.load
  const artifactsBusy = artifacts.busy
  const artifactsError = artifacts.error
  const artifactsList = artifacts.items
  const selectedArtifactId = artifacts.selectedId
  const setSelectedArtifactId = artifacts.setSelectedId
  const selectedArtifact = artifacts.selected
  const artifactTextPreview = artifacts.textPreview
  const artifactTextBusy = artifacts.textBusy
  const artifactTextError = artifacts.textError
  return (
    <Dialog
      open={artifactsDialogOpen}
      onOpenChange={artifacts.onOpenChange}
    >
      <DialogContent className="max-w-[980px]">
        <DialogTitle>Artifacts</DialogTitle>
        <DialogDescription className="mb-3">
          View files produced or referenced in this chat persona. Attachments stay channel-local; web can preview them here.
        </DialogDescription>
        <div className="flex gap-3 items-start flex-wrap flex-col md:flex-row">
          <div className="min-w-0 w-full flex-1 md:min-w-[250px]">
            <div className="flex justify-between items-center mb-2 gap-2 flex-wrap">
              <Select value={artifactKindFilter} onValueChange={setArtifactKindFilter}>
                <SelectTrigger className="w-[150px]" ><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All kinds</SelectItem>
                  <SelectItem value="image">Images</SelectItem>
                  <SelectItem value="markdown">Markdown</SelectItem>
                  <SelectItem value="html">HTML</SelectItem>
                  <SelectItem value="text">Text</SelectItem>
                  <SelectItem value="other">Other</SelectItem>
                </SelectContent>
              </Select>
              <Button size="sm" variant="secondary" onClick={() => void loadArtifacts(chatId, activePersonaId)} disabled={artifactsBusy}>
                Refresh
              </Button>
            </div>
            <div className={appearance === 'dark'
              ? 'max-h-[min(440px,65vh)] overflow-auto rounded-md border border-[color:var(--mc-border-soft)]'
              : 'max-h-[min(440px,65vh)] overflow-auto rounded-md border border-[color:var(--mc-border-strong)]'
            }>
              {artifactsBusy ? (
                <ArtifactListSkeleton />
              ) : artifactsError ? (
                <div role="status" className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive m-2">{artifactsError}</div>
              ) : artifactsList.length === 0 ? (
                <span className="block p-2">No artifacts found for this persona.</span>
              ) : (
                <ul className="list-none m-0 p-0">
                  {artifactsList.map((it) => (
                    <li key={it.id}>
                      <button
                        type="button"
                        onClick={() => setSelectedArtifactId(it.id)}
                        className={selectedArtifactId === it.id
                          ? 'w-full border-0 border-b text-left p-2 bg-[var(--accent-3)]'
                          : 'w-full border-0 border-b text-left p-2'}
                        style={appearance === 'dark' ? { borderBottomColor: 'var(--mc-border-soft)' } : { borderBottomColor: 'var(--gray-6)' }}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="truncate">{it.name}</span>
                          <span>{it.kind}</span>
                        </div>
                        <span>
                          {formatBytes(it.size_bytes ?? null)} · {it.source}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
          <div className={appearance === 'dark'
            ? 'min-h-[200px] min-w-0 w-full flex-[2] rounded-md border border-[color:var(--mc-border-soft)] p-2 md:min-w-[320px]'
            : 'min-h-[200px] min-w-0 w-full flex-[2] rounded-md border border-[color:var(--mc-border-strong)] p-2 md:min-w-[320px]'
          }>
            {selectedArtifact == null ? (
              <span>Select an artifact to preview.</span>
            ) : (
              <>
                <div className="flex justify-between items-center mb-2 flex-wrap gap-2">
                  <div>
                    <span className="font-semibold">{selectedArtifact.name}</span>
                    <span className="block">
                      {selectedArtifact.created_at ?? 'unknown time'} · {selectedArtifact.kind}
                    </span>
                  </div>
                  <div className="flex gap-2">
                    <Button size="sm" variant="secondary" onClick={() => window.open(selectedArtifact.url, '_blank', 'noopener,noreferrer')}>
                      Open
                    </Button>
                    <Button size="sm" variant="secondary" onClick={() => window.open(`${selectedArtifact.url}${selectedArtifact.url.includes('?') ? '&' : '?'}download=1`, '_blank', 'noopener,noreferrer')}>
                      Download
                    </Button>
                  </div>
                </div>
                {selectedArtifact.kind === 'image' ? (
                  <img src={artifactPreviewUrl(selectedArtifact)} alt={selectedArtifact.name} className="max-h-[56vh] w-full object-contain" />
                ) : selectedArtifact.kind === 'markdown' ? (
                  artifactTextBusy ? (
                    <ContentPreviewSkeleton />
                  ) : artifactTextError ? (
                    <div role="status" className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">{artifactTextError}</div>
                  ) : (
                    <div className="aui-md-root max-h-[56vh] overflow-auto text-sm leading-relaxed">
                      <ReactMarkdown
                        remarkPlugins={[remarkGfm]}
                        components={{
                          a: (props) => <MarkdownExternalLink {...props} />,
                        }}
                      >
                        {artifactTextPreview}
                      </ReactMarkdown>
                    </div>
                  )
                ) : selectedArtifact.kind === 'html' ? (
                  <iframe
                    title={selectedArtifact.name}
                    src={artifactPreviewUrl(selectedArtifact)}
                    sandbox="allow-same-origin"
                    className="h-[56vh] w-full rounded border border-[color:var(--gray-6)]"
                  />
                ) : selectedArtifact.kind === 'text' ? (
                  artifactTextBusy ? (
                    <ContentPreviewSkeleton />
                  ) : artifactTextError ? (
                    <div role="status" className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">{artifactTextError}</div>
                  ) : (
                    <pre className="max-h-[56vh] overflow-auto whitespace-pre-wrap text-xs">{artifactTextPreview}</pre>
                  )
                ) : (
                  <span>
                    Preview unavailable for this file type. Use Open or Download.
                  </span>
                )}
              </>
            )}
          </div>
        </div>
        <div className="flex justify-end mt-3">
          <DialogClose>
            <Button variant="secondary">Close</Button>
          </DialogClose>
        </div>
      </DialogContent>
    </Dialog>
    
  )
}
