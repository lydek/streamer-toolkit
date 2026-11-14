// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatEvent } from './chat'
import { loadTemplate, mount, parseTemplate } from './template'

const BASE = 'http://127.0.0.1:47810/templates/chat/'

const chat = (id: string, text = `msg ${id}`, color: string | null = null): ChatEvent => ({
  id,
  type: 'chat.message',
  version: 1,
  seq: 1,
  occurredAt: '2026-11-13T10:00:00.000Z',
  source: 'twitch',
  payload: { userId: id, userLogin: `user${id}`, userName: `User ${id}`, color, text, fragments: [] },
})

const TEMPLATE = `
<link rel="stylesheet" href="style.css">
<ol data-chat-list>
  <template data-chat-message><li><b data-field="userName"></b>: <span data-field="text"></span></li></template>
</ol>`

describe('parseTemplate', () => {
  it('removes scripts and event handlers, also inside the message template', () => {
    const t = parseTemplate(
      `<script>alert(1)</script><div data-chat-list onclick="alert(2)">
        <template data-chat-message><p onmouseover="alert(3)"><script src="x.js"></script><iframe srcdoc="x"></iframe>
        <span data-field="text"></span></p></template></div><meta http-equiv="refresh" content="0;url=https://evil.example">`,
      BASE,
      '1',
    )!
    const host = document.createElement('div')
    mount(host, t)([chat('a')])
    expect(host.innerHTML).not.toMatch(/script|iframe|meta|onclick|onmouseover|alert/)
    expect(host.textContent).toContain('msg a')
  })

  it('points relative URLs into the template folder with a fresh revision', () => {
    const t = parseTemplate(`${TEMPLATE}<img src="img/logo.png"><img src="https://cdn.example/x.png">`, BASE, '42')!
    const host = document.createElement('div')
    mount(host, t)
    expect(host.querySelector('link')!.getAttribute('href')).toBe(`${BASE}style.css?v=42`)
    const [local, remote] = host.querySelectorAll('img')
    expect(local.getAttribute('src')).toBe(`${BASE}img/logo.png?v=42`)
    expect(remote.getAttribute('src')).toBe('https://cdn.example/x.png')
  })

  it('returns null without the binding markers', () => {
    expect(parseTemplate('<ul></ul>', BASE, '1')).toBeNull()
    expect(parseTemplate('<ul data-chat-list></ul>', BASE, '1')).toBeNull()
    // Markers that only appear in a comment do not count.
    expect(parseTemplate('<!-- data-chat-list data-chat-message -->', BASE, '1')).toBeNull()
  })
})

describe('mount', () => {
  it('inserts viewer input as plain text and exposes the name color', () => {
    const host = document.createElement('div')
    mount(host, parseTemplate(TEMPLATE, BASE, '1')!)([chat('a', '<img src=x onerror=alert(1)>', '#FF0000')])
    expect(host.querySelector('img')).toBeNull()
    expect(host.querySelector('span')!.textContent).toBe('<img src=x onerror=alert(1)>')
    expect(host.querySelector('li')!.style.getPropertyValue('--user-color')).toBe('#FF0000')
  })

  it('keeps the nodes of messages already shown and drops old ones', () => {
    const host = document.createElement('div')
    const render = mount(host, parseTemplate(TEMPLATE, BASE, '1')!)
    render([chat('a'), chat('b')])
    const b = host.querySelectorAll('li')[1]
    render([chat('b'), chat('c')])
    const items = host.querySelectorAll('li')
    expect([...items].map((li) => li.querySelector('span')!.textContent)).toEqual(['msg b', 'msg c'])
    expect(items[0]).toBe(b)
  })

  it('redraws everything when a snapshot is not a continuation', () => {
    const host = document.createElement('div')
    const render = mount(host, parseTemplate(TEMPLATE, BASE, '1')!)
    render([chat('a'), chat('b')])
    render([chat('x'), chat('b')])
    expect([...host.querySelectorAll('span')].map((s) => s.textContent)).toEqual(['msg x', 'msg b'])
  })

  it('re-applies a new template with the same messages', () => {
    const host = document.createElement('div')
    const messages = [chat('a'), chat('b')]
    mount(host, parseTemplate(TEMPLATE, BASE, '1')!)(messages)
    mount(host, parseTemplate('<div data-chat-list><template data-chat-message><p data-field="text"></p></template></div>', BASE, '2')!)(messages)
    expect(host.querySelector('ol')).toBeNull()
    expect([...host.querySelectorAll('p')].map((p) => p.textContent)).toEqual(['msg a', 'msg b'])
  })
})

describe('loadTemplate', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('falls back to the default when the template is missing or broken', async () => {
    for (const res of [new Response('', { status: 404 }), new Response('<p>broken</p>')]) {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(res))
      const t = await loadTemplate(BASE, '1')
      expect(t.list.matches('[data-chat-list]')).toBe(true)
    }
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('offline')))
    expect((await loadTemplate(BASE, '1')).item.content.querySelector('[data-field="text"]')).not.toBeNull()
  })
})
