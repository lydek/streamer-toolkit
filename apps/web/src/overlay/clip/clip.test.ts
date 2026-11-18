// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { type ClipState, type View, embedUrl, render, view } from './clip'

const ID = 'CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4'
const big = { width: 1280, height: 720 }
const state = (s: ClipState['state'], sessionId = 'session-1'): ClipState => ({
  sessionId,
  state: s,
  clip: s === 'idle' ? null : { id: ID, durationMs: 30_000 },
})

describe('embedUrl', () => {
  it('builds the official embed for the page host', () => {
    const url = new URL(embedUrl(ID, '127.0.0.1'))
    expect(url.origin + url.pathname).toBe('https://clips.twitch.tv/embed')
    expect(Object.fromEntries(url.searchParams)).toEqual({
      clip: ID,
      parent: '127.0.0.1',
      autoplay: 'true',
      muted: 'false',
    })
  })
})

describe('view', () => {
  it('shows the embed only while loading or estimated playing', () => {
    expect(view(state('loading'), big, 'localhost')).toEqual({
      kind: 'embed',
      session: 'session-1',
      src: embedUrl(ID, 'localhost'),
    })
    expect(view(state('estimated_playing'), big, 'localhost').kind).toBe('embed')
    for (const s of ['idle', 'transitioning', 'stopped', 'error'] as const) {
      expect(view(state(s), big, 'localhost'), s).toEqual({ kind: 'none' })
    }
    expect(view(null, big, 'localhost')).toEqual({ kind: 'none' })
  })

  it('shows a notice instead of a player smaller than 400x300', () => {
    expect(view(state('loading'), { width: 399, height: 720 }, 'localhost')).toEqual({ kind: 'small' })
    expect(view(state('loading'), { width: 1280, height: 299 }, 'localhost')).toEqual({ kind: 'small' })
    expect(view(state('loading'), { width: 400, height: 300 }, 'localhost').kind).toBe('embed')
    expect(view(state('stopped'), { width: 100, height: 100 }, 'localhost')).toEqual({ kind: 'none' })
  })
})

describe('render', () => {
  const embed = (session: string): View => ({ kind: 'embed', session, src: embedUrl(ID, 'localhost') })

  it('keeps the iframe for the same session and replaces it for a new one', () => {
    const root = document.createElement('div')
    render(root, embed('a'), 'small')
    const first = root.firstElementChild as HTMLIFrameElement
    expect(first.tagName).toBe('IFRAME')
    expect(first.src).toBe(embedUrl(ID, 'localhost'))
    expect(first.allow).toBe('autoplay')

    render(root, embed('a'), 'small')
    expect(root.firstElementChild).toBe(first)
    render(root, embed('b'), 'small')
    expect(root.firstElementChild).not.toBe(first)
    expect(root.children).toHaveLength(1)
  })

  it('shows the notice as text alone and removes everything for none', () => {
    const root = document.createElement('div')
    render(root, embed('a'), '<b>small</b>')
    render(root, { kind: 'small' }, '<b>small</b>')
    expect(root.children).toHaveLength(1)
    expect(root.querySelector('iframe')).toBeNull()
    expect(root.textContent).toBe('<b>small</b>')

    render(root, embed('a'), 'small')
    expect(root.querySelector('iframe')).not.toBeNull()
    render(root, { kind: 'none' }, 'small')
    expect(root.children).toHaveLength(0)
  })
})
