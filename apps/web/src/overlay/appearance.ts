import type { components } from '../api/schema'
import type { Incoming } from './chat/chat'

export type Appearance = components['schemas']['AppearancePayload']

/*
 * The built-in overlays' theme and text size (screen tools 疊加層外觀), as CSS variables on the page root. Built-in
 * overlays and built-in templates read them with their own look as the fallback; custom templates never use them, so
 * they stay as written. A new built-in overlay follows by calling `followAppearance` and reading the same variables:
 * `--ov-fg` text, `--ov-bg` panels, `--ov-accent` highlights, `--ov-shadow` text shadow, `--ov-scale` text size.
 */
const THEMES: Record<Appearance['theme'], Record<string, string>> = {
  // How the overlays look without a theme.
  dark: {},
  cream: { '--ov-fg': '#3b2a1a', '--ov-bg': 'rgb(251 246 238 / 92%)', '--ov-accent': '#8a5a2b', '--ov-shadow': 'none' },
  contrast: { '--ov-fg': '#fff', '--ov-bg': '#000', '--ov-accent': '#ffd400', '--ov-shadow': '0 0 2px #000' },
}
const VARS = ['--ov-fg', '--ov-bg', '--ov-accent', '--ov-shadow', '--ov-scale']

export function applyAppearance(root: HTMLElement, a: Appearance | undefined) {
  for (const v of VARS) root.style.removeProperty(v)
  if (!a) return
  for (const [k, v] of Object.entries(THEMES[a.theme])) root.style.setProperty(k, v)
  if (a.textSize !== 100) root.style.setProperty('--ov-scale', String(a.textSize / 100))
}

/** Follows `screen.appearance` from snapshots and newer events; call it with every message an overlay receives. */
export function followAppearance(root: HTMLElement = document.documentElement) {
  let lastSeq = 0
  return (msg: Incoming) => {
    if (msg.type === 'snapshot') {
      lastSeq = msg.seq
      // A snapshot always lists it; tests may leave it out.
      applyAppearance(root, msg.payload['screen.appearance']?.at(-1)?.payload)
    } else if (msg.type === 'screen.appearance' && msg.seq > lastSeq) {
      lastSeq = msg.seq
      applyAppearance(root, msg.payload)
    }
  }
}
