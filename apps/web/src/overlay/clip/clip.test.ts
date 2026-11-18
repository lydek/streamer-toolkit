// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { CAPTION_HEIGHT, type ClipState, type View, embedUrl, render, view } from './clip'

const ID = 'CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4'
const big = { width: 1280, height: 720 }
const state = (s: ClipState['state'], sessionId = 'session-1', title = 'F1 2017 E3 Gameplay!'): ClipState => ({
  sessionId,
  state: s,
  clip: s === 'idle' ? null : { id: ID, title, broadcasterName: 'Twitch', durationMs: 30_000 },
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
  it('shows the embed and its caption only while loading or estimated playing', () => {
    expect(view(state('loading'), big, 'localhost')).toEqual({
      kind: 'embed',
      session: 'session-1',
      src: embedUrl(ID, 'localhost'),
      channel: 'Twitch',
      title: 'F1 2017 E3 Gameplay!',
    })
    expect(view(state('estimated_playing'), big, 'localhost').kind).toBe('embed')
    for (const s of ['idle', 'transitioning', 'stopped', 'error'] as const) {
      expect(view(state(s), big, 'localhost'), s).toEqual({ kind: 'none' })
    }
    expect(view(null, big, 'localhost')).toEqual({ kind: 'none' })
  })

  it('keeps the player at least 400x300 after the caption takes its height', () => {
    const tall = 300 + CAPTION_HEIGHT
    expect(view(state('loading'), { width: 399, height: 720 }, 'localhost')).toEqual({ kind: 'small' })
    expect(view(state('loading'), { width: 1280, height: tall - 1 }, 'localhost')).toEqual({ kind: 'small' })
    expect(view(state('loading'), { width: 400, height: tall }, 'localhost').kind).toBe('embed')
    expect(view(state('stopped'), { width: 100, height: 100 }, 'localhost')).toEqual({ kind: 'none' })
  })
})

describe('render', () => {
  const embed = (session: string, title = 'Clip'): View => ({
    kind: 'embed',
    session,
    src: embedUrl(ID, 'localhost'),
    channel: 'Twitch',
    title,
  })

  it('keeps the iframe for the same session and replaces it for a new one', () => {
    const root = document.createElement('div')
    render(root, embed('a'), 'small')
    const first = root.firstElementChild as HTMLIFrameElement
    expect(first.tagName).toBe('IFRAME')
    expect(first.src).toBe(embedUrl(ID, 'localhost'))
    expect(first.allow).toBe('autoplay')

    render(root, embed('a', 'Renamed'), 'small')
    expect(root.firstElementChild).toBe(first)
    expect(root.querySelector('.title')?.textContent).toBe('Renamed')
    render(root, embed('b'), 'small')
    expect(root.firstElementChild).not.toBe(first)
    expect(root.children).toHaveLength(2)
  })

  it('puts the caption after the player, as text only', () => {
    const root = document.createElement('div')
    render(root, embed('a', '<img src=x onerror=alert(1)>'), 'small')
    const [frame, caption] = [...root.children]
    expect(frame.tagName).toBe('IFRAME')
    expect(caption.className).toBe('caption')
    expect(frame.contains(caption)).toBe(false)
    expect(caption.querySelector('.channel')?.textContent).toBe('Twitch')
    expect(caption.querySelector('.title')?.textContent).toBe('<img src=x onerror=alert(1)>')
    expect(caption.querySelector('img')).toBeNull()
  })

  it('shows the notice as text alone and removes everything for none', () => {
    const root = document.createElement('div')
    render(root, embed('a'), '<b>small</b>')
    render(root, { kind: 'small' }, '<b>small</b>')
    expect(root.children).toHaveLength(1)
    expect(root.querySelector('iframe, .caption')).toBeNull()
    expect(root.textContent).toBe('<b>small</b>')

    render(root, embed('a'), 'small')
    expect(root.querySelector('iframe')).not.toBeNull()
    render(root, { kind: 'none' }, 'small')
    expect(root.children).toHaveLength(0)
  })
})
