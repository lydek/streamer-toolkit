import type { components } from '../../api/schema'

export type ClipState = components['schemas']['ClipStatePayload']

/** Twitch's smallest embed; a smaller OBS source gets a notice instead of a cropped player. */
export const MIN_WIDTH = 400
export const MIN_HEIGHT = 300
/** The caption strip under the player, in CSS pixels (matches `.caption` in index.html). */
export const CAPTION_HEIGHT = 72

/** What the page shows: the official embed and its caption for one session, the too-small notice, or nothing. */
export type View =
  | { kind: 'embed'; session: string; src: string; channel: string; title: string }
  | { kind: 'small' }
  | { kind: 'none' }

/** The official clip embed. `parent` is the host name the page was loaded from (`127.0.0.1` or `localhost`). */
export function embedUrl(clipId: string, parent: string): string {
  const q = new URLSearchParams({ clip: clipId, parent, autoplay: 'true', muted: 'false' })
  return `https://clips.twitch.tv/embed?${q}`
}

export function view(state: ClipState | null, size: { width: number; height: number }, parent: string): View {
  const playing = state?.state === 'loading' || state?.state === 'estimated_playing'
  if (!playing || !state.clip || !state.sessionId) return { kind: 'none' }
  // The caption sits below the player, never on it, so the player gets what is left of the height.
  if (size.width < MIN_WIDTH || size.height - CAPTION_HEIGHT < MIN_HEIGHT) return { kind: 'small' }
  return {
    kind: 'embed',
    session: state.sessionId,
    src: embedUrl(state.clip.id, parent),
    channel: state.clip.broadcasterName,
    title: state.clip.title,
  }
}

function line(className: string, text: string) {
  const el = document.createElement('div')
  el.className = className
  el.textContent = text
  return el
}

/**
 * Puts `v` in `root`. The same session keeps its iframe, so a reconnect or repeated state never restarts the clip.
 * The iframe's `load` event says nothing about playback and is not listened to. Twitch text goes in as text only.
 */
export function render(root: HTMLElement, v: View, notice: string) {
  const current = root.firstElementChild as HTMLElement | null
  if (v.kind === 'embed' && current instanceof HTMLIFrameElement && current.dataset.session === v.session) {
    const caption = root.querySelector('.caption')
    caption?.replaceChildren(line('channel', v.channel), line('title', v.title))
    return
  }
  if (v.kind === 'small' && current?.dataset.notice !== undefined) return
  if (v.kind === 'none' && !current) return
  root.replaceChildren()
  if (v.kind === 'embed') {
    const frame = document.createElement('iframe')
    frame.dataset.session = v.session
    frame.src = v.src
    frame.allow = 'autoplay'
    frame.title = 'Twitch clip'
    const caption = document.createElement('div')
    caption.className = 'caption'
    caption.append(line('channel', v.channel), line('title', v.title))
    root.append(frame, caption)
  } else if (v.kind === 'small') {
    const p = document.createElement('p')
    p.dataset.notice = ''
    p.textContent = notice
    root.append(p)
  }
}
