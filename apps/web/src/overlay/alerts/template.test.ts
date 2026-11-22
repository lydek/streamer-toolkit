// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import type { AlertPayload } from './alerts'
import { fillAlert, loadAlertTemplate, parseAlertTemplate } from './template'
import defaultHtml from './templates/default/template.html?raw'

const t = i18n.global.t

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
    ).map((p) => render({ ...p, userId: '7', userLogin: 'viewer', userName: name, durationMs: 6000, sound: { kind: 'built_in', volume: 100 } }).querySelector('.title')!.textContent)
    expect(titles).toEqual([
      t('alerts.text.follow', { name }),
      t('alerts.text.subscribe', { name }),
      t('alerts.text.resubscribe', { name, months: 12 }),
      t('alerts.text.gift', { name, count: 20 }),
      t('alerts.text.cheer', { name, bits: 100 }),
      t('alerts.text.raid', { name, viewers: 42 }),
    ])
  })

  it('names an anonymous gift, keeps a message as text and drops an empty one', () => {
    const gift = render({ kind: 'gift', userId: null, userLogin: null, userName: null, tier: 1, count: 5, durationMs: 6000, sound: { kind: 'built_in', volume: 100 } })
    expect(gift.querySelector('.title')!.textContent).toBe(t('alerts.text.gift', { name: t('alerts.anonymous'), count: 5 }))
    expect(gift.querySelector('.message')).toBeNull()

    const cheer = render({ kind: 'cheer', userId: '7', userLogin: 'a', userName: 'A', bits: 1, message: '<b>hi</b>', durationMs: 9000, sound: { kind: 'built_in', volume: 100 } })
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
