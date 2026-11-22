// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../../i18n'
import type { Incoming } from '../chat/chat'
import type { AlertEvent } from './alerts'
import AlertsOverlay from './AlertsOverlay.vue'
import chimeUrl from './chime.wav'

const t = i18n.global.t

const played = vi.hoisted(() => [] as [string, number][])
vi.mock('../sound', () => ({ playSound: (url: string, volume: number) => played.push([url, volume]), stopSounds: () => {} }))

// Each load waits until the test releases it with the stage class it should get.
const loads = vi.hoisted(() => [] as { base: string; release: (stage: string) => void }[])

vi.mock('./template', async (original) => {
  const real = await original<typeof import('./template')>()
  return {
    ...real,
    loadAlertTemplate: (base: string, rev: string) =>
      new Promise((done) =>
        loads.push({
          base,
          release: (stage) =>
            done(
              real.parseAlertTemplate(
                `<div class="${stage}" data-alert-stage><template data-alert><p data-field="title"></p></template></div>`,
                base,
                rev,
              ),
            ),
        }),
      ),
  }
})

const alert = (seq: number, durationMs = 6000): AlertEvent => ({
  id: `a${seq}`,
  type: 'alert',
  version: 1,
  seq,
  occurredAt: '',
  source: 'simulation',
  payload: { kind: 'raid', userId: `${seq}`, userLogin: `r${seq}`, userName: `R${seq}`, viewers: seq, durationMs, sound: { kind: 'built_in', volume: 100 } },
})
const changed: Incoming = {
  type: 'template.changed',
  id: 't',
  version: 1,
  seq: 0,
  occurredAt: '',
  source: 'system',
  payload: { overlay: 'alerts' },
}
const post = (data: Incoming, init: MessageEventInit = {}) =>
  window.dispatchEvent(new MessageEvent('message', { data, source: window.parent, origin: location.origin, ...init }))
const titles = () => [...document.querySelectorAll('[data-alert-stage] [data-field="title"]')].map((t) => t.textContent)

function preview(query = '?preview') {
  vi.stubGlobal('WebSocket', vi.fn())
  history.replaceState(null, '', `/overlay/alerts/${query}`)
  loads.length = 0
  return mount(AlertsOverlay, { global: { plugins: [i18n] }, attachTo: document.body })
}

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  history.replaceState(null, '', '/')
})

describe('AlertsOverlay', () => {
  it('an emergency stop also drops an alert still waiting for its template', async () => {
    const wrapper = preview()
    post(alert(1))
    post({ type: 'presentation.stopped', id: 's', version: 1, seq: 2, occurredAt: '', source: 'system', payload: { stopped: true, cancelled: 0 } })
    for (const l of loads) l.release('a')
    await flushPromises()
    expect(titles()).toEqual([])
    wrapper.unmount()
  })

  it('previews the built-in template named in the URL, and only a well-formed name', async () => {
    for (const [query, base] of [
      ['?preview&template=banner', '/templates/built-in/alerts/banner/'],
      ['?preview&template=../x', '/templates/alerts/'],
    ]) {
      const wrapper = preview(query)
      expect(loads.map((l) => l.base)).toEqual([`${location.origin}${base}`])
      wrapper.unmount()
    }
  })

  it('plays each alert’s sound: the built-in one, a library sound at its volume, or none', async () => {
    vi.useFakeTimers()
    const sockets: { onmessage?: (e: { data: string }) => void }[] = []
    vi.stubGlobal(
      'WebSocket',
      class {
        onmessage?: (e: { data: string }) => void
        constructor() {
          sockets.push(this)
        }
        close() {}
      },
    )
    history.replaceState(null, '', '/overlay/alerts/')
    loads.length = 0
    played.length = 0
    const wrapper = mount(AlertsOverlay, { global: { plugins: [i18n] }, attachTo: document.body })
    const send = (m: unknown) => sockets[0].onmessage!({ data: JSON.stringify(m) })
    send({ type: 'snapshot', version: 1, seq: 0, payload: {} })
    for (const l of loads) l.release('a')
    await vi.runAllTimersAsync()
    const sounds: AlertEvent['payload']['sound'][] = [
      { kind: 'built_in', volume: 70 },
      { kind: 'file', url: '/sounds/s1', volume: 40 },
      { kind: 'silent' },
    ]
    for (const [i, sound] of sounds.entries()) {
      send({ ...alert(i + 1, 10), payload: { ...alert(i + 1, 10).payload, sound } })
      await vi.runAllTimersAsync()
    }
    expect(played).toEqual([
      [chimeUrl, 70],
      ['/sounds/s1', 40],
    ])
    wrapper.unmount()
  })

  it('shows a reward video at its place and volume for its time, and drops it on a skip', async () => {
    vi.useFakeTimers()
    const sockets: { onmessage?: (e: { data: string }) => void }[] = []
    vi.stubGlobal(
      'WebSocket',
      class {
        onmessage?: (e: { data: string }) => void
        constructor() {
          sockets.push(this)
        }
        close() {}
      },
    )
    history.replaceState(null, '', '/overlay/alerts/')
    const wrapper = mount(AlertsOverlay, { global: { plugins: [i18n] }, attachTo: document.body })
    const send = async (m: unknown) => {
      sockets[0].onmessage!({ data: JSON.stringify(m) })
      await flushPromises()
    }
    const media = (seq: number, format: string, durationMs: number) => ({
      id: `m${seq}`, type: 'screen.media', version: 1, seq, occurredAt: '', source: 'twitch',
      payload: { url: '/media/m1', format, position: 'bottom_right', volume: 40, durationMs },
    })
    await send({ type: 'snapshot', version: 1, seq: 0, payload: {} })
    await send(media(1, 'webm', 3000))
    const shown = wrapper.get('[data-media]')
    expect(shown.classes()).toContain('at-bottom_right')
    expect((shown.get('video').element as HTMLVideoElement).volume).toBeCloseTo(0.4)
    await vi.advanceTimersByTimeAsync(3000)
    expect(wrapper.find('[data-media]').exists()).toBe(false)

    await send(media(2, 'gif', 5000))
    expect(wrapper.get('[data-media] img').attributes('src')).toBe('/media/m1')
    await send({ type: 'presentation.skipped', id: 'k', version: 1, seq: 3, occurredAt: '', source: 'system', payload: { stage: 'alerts' } })
    expect(wrapper.find('[data-media]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('in preview, takes only messages from the parent on the same origin', async () => {
    const wrapper = preview()
    loads[0].release('a')
    post(alert(1), { origin: 'http://evil.example' })
    post(alert(2), { source: null })
    await flushPromises()
    expect(titles()).toEqual([])
    post(alert(3))
    await flushPromises()
    expect(titles()).toEqual([t('alerts.text.raid', { name: 'R3', viewers: 3 })])
    wrapper.unmount()
  })

  it('an alert arriving while a template change supersedes a load still plays, in the newest template', async () => {
    const wrapper = preview()
    post(changed) // the first load is superseded before it finishes
    post(alert(1))
    loads[0].release('old')
    await flushPromises()
    expect(titles()).toEqual([])
    loads[1].release('new')
    await flushPromises()
    expect(document.querySelectorAll('.new [data-field="title"]')).toHaveLength(1)
    expect(document.querySelector('.old')).toBeNull()
    wrapper.unmount()
  })

  it('an alert on screen carries on in a new template for the rest of its time', async () => {
    vi.useFakeTimers()
    const wrapper = preview()
    loads[0].release('old')
    post(alert(1, 6000))
    await flushPromises()
    expect(titles()).toEqual([t('alerts.text.raid', { name: 'R1', viewers: 1 })])

    vi.advanceTimersByTime(2000)
    post(changed)
    loads[1].release('new')
    await flushPromises()
    const shown = document.querySelector<HTMLElement>('.new [data-kind]')!
    expect(shown.textContent).toBe(t('alerts.text.raid', { name: 'R1', viewers: 1 }))
    expect(shown.style.getPropertyValue('--duration')).toBe('4000ms')
    expect(document.querySelector('.old')).toBeNull()

    vi.advanceTimersByTime(4000)
    expect(titles()).toEqual([])
    wrapper.unmount()
  })
})
