import type { components } from '../../api/schema'
import { clean } from '../chat/template'
import { twitchImage } from './checkin'

type Stamped = components['schemas']['CheckinStampedPayload']

/**
 * A stamp card animation pack (`templates/checkin/<id>/index.html`, no script): the nodes to mount, where the card
 * shows (`[data-card-stage]`), and the markup for one card (`<template data-card>`). In the card markup,
 * `data-field` names what goes in: `title`, `description`, `name`, `count` (stamps/slots), `banner` (full cards
 * only), `avatar` (an `<img>`) and `slots` (one `<li>` per slot, `stamped` and `latest` as classes).
 */
export interface CardPack {
  nodes: Node[]
  stage: Element
  item: HTMLTemplateElement
}

/** The pack a `pack:<id>` preset names. */
export const packOf = (preset: string | undefined) => preset?.match(/^pack:([0-9a-f-]{36})$/)?.[1]

/** Parses inertly (DOMParser runs nothing); `null` when the binding markers are missing. */
export function parsePack(html: string, base: string, rev: string): CardPack | null {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  clean(doc, base, rev)
  const stage = doc.querySelector('[data-card-stage]')
  const item = doc.querySelector('template[data-card]')
  if (!stage || !(item instanceof HTMLTemplateElement)) return null
  item.remove()
  return { nodes: [...doc.head.childNodes, ...doc.body.childNodes], stage, item }
}

/** Fetches the pack; `null` when it is gone or broken, so the built-in card shows instead. */
export async function loadPack(id: string, rev = String(Date.now())): Promise<CardPack | null> {
  const base = `${location.origin}/templates/checkin/${id}/`
  try {
    const res = await fetch(new URL('index.html', base), { cache: 'no-store' })
    return res.ok ? parsePack(await res.text(), base, rev) : null
  } catch {
    return null
  }
}

/** One card's markup filled in. Viewer text only ever goes in as text, never as markup. */
export function fillPack(item: HTMLTemplateElement, p: Stamped): DocumentFragment {
  const frag = item.content.cloneNode(true) as DocumentFragment
  const text: Record<string, string> = {
    title: p.title ?? p.userName,
    description: p.description ?? '',
    name: p.userName,
    count: `${p.stamps}/${p.slots}`,
  }
  for (const el of frag.querySelectorAll<HTMLElement>('[data-field]')) {
    const field = el.dataset.field!
    if (field in text) el.textContent = text[field]
    else if (field === 'banner' && !p.full) el.remove()
    else if (field === 'avatar') {
      const url = p.avatarUrl ? twitchImage(p.avatarUrl) : undefined
      if (url && el instanceof HTMLImageElement) el.src = url
      else el.remove()
    } else if (field === 'slots') {
      el.replaceChildren(
        ...Array.from({ length: p.slots }, (_, i) => {
          const li = document.createElement('li')
          if (i < p.stamps) li.classList.add('stamped')
          if (i + 1 === p.stamps) li.classList.add('latest')
          return li
        }),
      )
    }
  }
  for (const el of frag.children) {
    const root = el as HTMLElement
    root.dataset.full = String(p.full)
    root.style.setProperty('--duration', `${p.durationMs}ms`)
    if (p.style) {
      root.style.setProperty('--card-bg', p.style.backgroundColor)
      root.style.setProperty('--stamp', p.style.stampColor)
      root.style.setProperty('--per-row', String(p.style.perRow))
    }
  }
  return frag
}
