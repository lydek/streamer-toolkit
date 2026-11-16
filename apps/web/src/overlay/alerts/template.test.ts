// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AlertPayload } from './alerts'
import { fillAlert, loadAlertTemplate, parseAlertTemplate } from './template'
import defaultHtml from './templates/default/template.html?raw'

const BASE = 'http://127.0.0.1:47810/templates/alerts/'

function render(p: AlertPayload) {
  const { item } = parseAlertTemplate(defaultHtml, BASE, '1')!
  const host = document.createElement('div')
  host.append(fillAlert(item, p))
  return host.firstElementChild as HTMLElement
}

describe('fillAlert', () => {
  it('says what happened for each kind, with the name as plain text', () => {
    const name = '<img src=x onerror=alert(1)>'
    const titles = (
      [
        { kind: 'follow' },
        { kind: 'subscribe', tier: 1 },
        { kind: 'resubscribe', tier: 1, months: 12 },
        { kind: 'gift', tier: 1, count: 20 },
        { kind: 'cheer', bits: 100 },
        { kind: 'raid', viewers: 42 },
      ] as const
    ).map((p) => render({ ...p, userName: name, durationMs: 6000, sound: { kind: 'built_in' } }).querySelector('.title')!.textContent)
    expect(titles).toEqual([
      `${name} 追隨了！`,
      `${name} 訂閱了！`,
      `${name} 續訂了 12 個月！`,
      `${name} 送了 20 份訂閱！`,
      `${name} 送出 100 小奇點！`,
      `${name} 帶著 42 人揪團來了！`,
    ])
  })

  it('names an anonymous gift, keeps a message as text and drops an empty one', () => {
    const gift = render({ kind: 'gift', userName: null, tier: 1, count: 5, durationMs: 6000, sound: { kind: 'built_in' } })
    expect(gift.querySelector('.title')!.textContent).toBe('匿名觀眾 送了 5 份訂閱！')
    expect(gift.querySelector('.message')).toBeNull()

    const cheer = render({ kind: 'cheer', userName: 'A', bits: 1, message: '<b>hi</b>', durationMs: 9000, sound: { kind: 'built_in' } })
    expect(cheer.querySelector('.message')!.innerHTML).toBe('&lt;b&gt;hi&lt;/b&gt;')
    expect(cheer.dataset.kind).toBe('cheer')
    expect(cheer.style.getPropertyValue('--duration')).toBe('9000ms')
  })
})

describe('loadAlertTemplate', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('points the stylesheet into the template folder, and falls back to the default when the fetch fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))
    const t = await loadAlertTemplate(BASE, '7')
    const link = t.nodes.find((n): n is HTMLLinkElement => n instanceof HTMLLinkElement)!
    expect(link.href).toBe(`${BASE}style.css?v=7`)
    expect(t.stage.matches('[data-alert-stage]')).toBe(true)
  })

  it('falls back when the markers are missing', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<p>no stage</p>')))
    expect((await loadAlertTemplate(BASE, '1')).item.content.querySelector('[data-field="title"]')).not.toBeNull()
  })
})
