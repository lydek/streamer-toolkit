import type { ChatEvent } from './chat'
import fallbackHtml from './default-template/template.html?raw'

/** A parsed user template: the nodes to mount, where messages go, and the markup for one message. */
export interface ChatTemplate {
  nodes: Node[]
  list: Element
  item: HTMLTemplateElement
}

// The Overlay CSP already blocks inline script; these are removed so nothing active or navigating is mounted.
const BLOCKED = 'script, iframe, frame, frameset, object, embed, base, meta, portal'
const URL_ATTRS = new Set(['src', 'href', 'xlink:href', 'poster', 'action', 'formaction'])
const SAFE_PROTOCOLS = new Set(['http:', 'https:', 'data:'])

/** Removes active content and points relative URLs into the template folder (`base`), tagged with `rev`. */
function clean(root: ParentNode, base: string, rev: string) {
  root.querySelectorAll(BLOCKED).forEach((el) => el.remove())
  for (const el of root.querySelectorAll('*')) {
    for (const attr of [...el.attributes]) {
      const { name, value } = attr
      if (name.startsWith('on') || name === 'srcdoc') {
        el.removeAttribute(name)
      } else if (URL_ATTRS.has(name) && !value.trim().startsWith('#')) {
        // Same-document fragments (SVG <use href="#icon">) stay as they are.
        try {
          const url = new URL(value, base)
          // javascript: and other schemes are dropped too, not only blocked by the Overlay CSP.
          if (!SAFE_PROTOCOLS.has(url.protocol)) throw new Error(url.protocol)
          // A fresh query per load makes OBS fetch edited files instead of reusing cached ones.
          if (url.href.startsWith(base)) url.searchParams.set('v', rev)
          // Set through the Attr so namespaced attributes (xlink:href) keep their namespace.
          attr.value = url.href
        } catch {
          el.removeAttribute(name)
        }
      }
    }
    // querySelectorAll does not look inside <template> content.
    if (el instanceof HTMLTemplateElement) clean(el.content, base, rev)
  }
}

/** Parses inertly (DOMParser runs nothing); `null` when the binding markers are missing. */
export function parseTemplate(html: string, base: string, rev: string): ChatTemplate | null {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  clean(doc, base, rev)
  const list = doc.querySelector('[data-chat-list]')
  const item = doc.querySelector('template[data-chat-message]')
  if (!list || !(item instanceof HTMLTemplateElement)) return null
  item.remove()
  return { nodes: [...doc.head.childNodes, ...doc.body.childNodes], list, item }
}

/** Fetches `<base>template.html`; a missing or broken template falls back to the shipped default, never blank. */
export async function loadTemplate(base: string, rev: string): Promise<ChatTemplate> {
  try {
    const res = await fetch(new URL('template.html', base), { cache: 'no-store' })
    const parsed = res.ok ? parseTemplate(await res.text(), base, rev) : null
    if (parsed) return parsed
  } catch {
    // Fall through to the default.
  }
  return parseTemplate(fallbackHtml, base, rev)!
}

function fill(item: HTMLTemplateElement, m: ChatEvent): DocumentFragment {
  const frag = item.content.cloneNode(true) as DocumentFragment
  // Viewer input only ever goes in as text, never as markup.
  for (const el of frag.querySelectorAll<HTMLElement>('[data-field]')) {
    if (el.dataset.field === 'userName') el.textContent = m.payload.userName
    else if (el.dataset.field === 'text') el.textContent = m.payload.text
  }
  if (m.payload.color) {
    for (const el of frag.children) (el as HTMLElement).style.setProperty('--user-color', m.payload.color)
  }
  return frag
}

/** Resolves once the stylesheets under `root` have loaded or failed, or after `timeoutMs`. */
export function stylesheetsLoaded(root: ParentNode, timeoutMs = 3000): Promise<void> {
  const pending = [...root.querySelectorAll<HTMLLinkElement>('link[rel~="stylesheet"]')]
    .filter((link) => !link.sheet)
    .map(
      (link) =>
        new Promise<void>((done) => {
          link.addEventListener('load', () => done(), { once: true })
          link.addEventListener('error', () => done(), { once: true })
        }),
    )
  return Promise.race([Promise.all(pending).then(() => {}), new Promise<void>((done) => setTimeout(done, timeoutMs))])
}

/** Mounts `template` into `host`; the returned renderer adds and removes only the messages that changed. */
export function mount(host: Element, { nodes, list, item }: ChatTemplate): (messages: ChatEvent[]) => void {
  host.replaceChildren(...nodes)
  let shown: { id: string; nodes: ChildNode[] }[] = []
  const drop = (s: { nodes: ChildNode[] }) => s.nodes.forEach((n) => n.remove())
  return (messages) => {
    const keep = new Set(messages.map((m) => m.id))
    for (const s of shown) if (!keep.has(s.id)) drop(s)
    shown = shown.filter((s) => keep.has(s.id))
    // Messages only append or drop from the front; anything else (a new snapshot) redraws them all.
    if (!shown.every((s, i) => s.id === messages[i].id)) {
      shown.forEach(drop)
      shown = []
    }
    for (const m of messages.slice(shown.length)) {
      const frag = fill(item, m)
      shown.push({ id: m.id, nodes: [...frag.childNodes] })
      list.append(frag)
    }
  }
}
