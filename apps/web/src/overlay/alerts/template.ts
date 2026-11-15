import { clean } from '../chat/template'
import { type AlertPayload, alertTitle } from './alerts'
import fallbackHtml from './templates/default/template.html?raw'

/** A parsed alert template: the nodes to mount, where an alert shows, and the markup for one alert. */
export interface AlertTemplate {
  nodes: Node[]
  stage: Element
  item: HTMLTemplateElement
}

/** Parses inertly (DOMParser runs nothing); `null` when the binding markers are missing. */
export function parseAlertTemplate(html: string, base: string, rev: string): AlertTemplate | null {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  clean(doc, base, rev)
  const stage = doc.querySelector('[data-alert-stage]')
  const item = doc.querySelector('template[data-alert]')
  if (!stage || !(item instanceof HTMLTemplateElement)) return null
  item.remove()
  return { nodes: [...doc.head.childNodes, ...doc.body.childNodes], stage, item }
}

/** Fetches `<base>template.html`; a missing or broken one falls back to the shipped default, never blank. */
export async function loadAlertTemplate(base: string, rev: string): Promise<AlertTemplate> {
  try {
    const res = await fetch(new URL('template.html', base), { cache: 'no-store' })
    const parsed = res.ok ? parseAlertTemplate(await res.text(), base, rev) : null
    if (parsed) return parsed
  } catch {
    // Fall through to the default.
  }
  return parseAlertTemplate(fallbackHtml, base, rev)!
}

/** One alert's markup filled in. Viewer input only ever goes in as text, never as markup. */
export function fillAlert(item: HTMLTemplateElement, p: AlertPayload): DocumentFragment {
  const frag = item.content.cloneNode(true) as DocumentFragment
  for (const el of frag.querySelectorAll<HTMLElement>('[data-field]')) {
    if (el.dataset.field === 'title') el.textContent = alertTitle(p)
    else if (el.dataset.field === 'message' && p.message) el.textContent = p.message
    else if (el.dataset.field === 'message') el.remove()
  }
  for (const el of frag.children) {
    ;(el as HTMLElement).dataset.kind = p.kind
    ;(el as HTMLElement).style.setProperty('--duration', `${p.durationMs}ms`)
  }
  return frag
}
