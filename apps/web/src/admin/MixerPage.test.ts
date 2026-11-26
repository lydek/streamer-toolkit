// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import Select from 'primevue/select'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import MixerPage from './MixerPage.vue'
import { mixPick } from './mixer'
import { testRouter } from './testRouter'

const { toastAdd } = vi.hoisted(() => ({ toastAdd: vi.fn() }))
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const t = i18n.global.t
const ok = () => Promise.resolve({ data: undefined, response: new Response(null, { status: 204 }) })

let socket: { onmessage?: (e: { data: string }) => void }
let seq = 0
const send = (msg: object) => socket.onmessage!({ data: JSON.stringify({ seq: ++seq, ...msg }) })
const input = (name: string, scenes: string[], extra: object = {}) => ({ name, muted: false, volumeDb: -6, monitor: 'off', scenes, ...extra })
const state = (live: string) => ({
  scene: live,
  scenes: [
    { name: 'Game', items: [] },
    { name: 'Chat', items: [] },
  ],
  inputs: [input('Mic', [], { global: true }), input('Game audio', ['Game']), input('Discord', ['Chat'], { muted: true })],
  streaming: false,
  recording: false,
})
function snapshot(obs: object) {
  send({
    type: 'snapshot',
    payload: {
      'obs.status': [],
      'obs.state': [{ payload: obs }],
      'presentation.queue': [],
      'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [],
      'chat.message': [],
    },
  })
}

let put: ReturnType<typeof vi.fn>

async function page() {
  const wrapper = mount(MixerPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return wrapper
}
const names = (el: Element | undefined) => [...(el?.querySelectorAll('.mix-name b') ?? [])].map((b) => b.textContent)

beforeEach(() => {
  seq = 0
  mixPick.value = ''
  vi.stubGlobal(
    'WebSocket',
    class {
      onmessage?: (e: { data: string }) => void
      constructor() {
        // eslint-disable-next-line @typescript-eslint/no-this-alias
        socket = this
      }
      close() {}
    },
  )
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  put = vi.fn(ok)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
  const data = (d: unknown) => Promise.resolve({ data: d, response: new Response(null, { status: 200 }) })
  vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
    data(path === '/api/admin/mixer/mic' ? { input: 'Mic' } : [{ name: 'Game audio', peakDb: -30 }, { name: 'Discord', peakDb: -6 }])) as never)
  toastAdd.mockReset()
})

enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('MixerPage', () => {
  it('shows OBS levels apart from the faders, empty while muted, and saves the mic choice', async () => {
    const wrapper = await page()
    snapshot(state('Game'))
    await flushPromises()
    const cover = (input: string) =>
      (wrapper.findAll('.mix-row').find((r) => r.get('.mix-name b').text() === input)!.get('.mix-meter i').element as HTMLElement).style.width
    expect(cover('Game audio')).toBe('50%')
    // Discord is muted: its level does not show.
    expect(cover('Discord')).toBe('100%')
    expect(cover('Mic')).toBe('100%')
    expect(wrapper.get('.mix-name .pi-microphone').element.closest('b')!.textContent).toBe('Mic')

    const micPicker = wrapper.findAllComponents(Select).find((s) => s.props('inputId') === 'mix-mic')!
    micPicker.vm.$emit('update:modelValue', 'Game audio')
    await flushPromises()
    expect(put).toHaveBeenCalledWith('/api/admin/mixer/mic', { body: { input: 'Game audio' } })
    expect(wrapper.get('.mix-name .pi-microphone').element.closest('b')!.textContent).toBe('Game audio')
    micPicker.vm.$emit('update:modelValue', 'Mic')
    await flushPromises()
  })

  it('lists the live scene inputs first, follows the live scene, and keeps a picked one', async () => {
    const wrapper = await page()
    snapshot(state('Game'))
    await flushPromises()
    const list = () => wrapper.element.querySelector('.mix-list')!
    const off = () => wrapper.element.querySelector('.mix-off .mix-list') ?? undefined
    expect(names(list())).toEqual(['Mic', 'Game audio'])
    expect(names(off())).toEqual(['Discord'])
    expect(wrapper.text()).toContain(t('mixer.global'))

    send({ type: 'obs.state', payload: state('Chat') })
    await flushPromises()
    expect(names(list())).toEqual(['Mic', 'Discord'])

    // Picked: stays on Game while the live scene changes, until 「跟著直播中」 is picked again.
    wrapper.findComponent(Select).vm.$emit('update:modelValue', 'Game')
    send({ type: 'obs.state', payload: state('Chat') })
    await flushPromises()
    expect(names(list())).toEqual(['Mic', 'Game audio'])
    wrapper.findComponent(Select).vm.$emit('update:modelValue', '')
    await flushPromises()
    expect(names(list())).toEqual(['Mic', 'Discord'])
  })

  it('lets the fader reach a level OBS has above 0 dB', async () => {
    const wrapper = await page()
    const loud = state('Game')
    loud.inputs[1] = input('Game audio', ['Game'], { volumeDb: 6 })
    snapshot(loud)
    await flushPromises()
    const fader = wrapper.get(`[aria-label="${t('mixer.volume', { input: 'Game audio' })}"]`).element as HTMLInputElement
    expect(fader.max).toBe('6')
    expect(fader.value).toBe('6')
  })

  it('sends mute, fader and monitoring changes for the input', async () => {
    const wrapper = await page()
    snapshot(state('Game'))
    await flushPromises()
    await wrapper.get(`[aria-label="${t('mixer.mute', { input: 'Mic' })}"]`).trigger('click')
    const fader = wrapper.get(`[aria-label="${t('mixer.volume', { input: 'Game audio' })}"]`)
    await fader.setValue('-12')
    await fader.trigger('change')
    // The floor is silent.
    await fader.setValue('-60')
    await fader.trigger('change')
    const monitor = wrapper.findAllComponents(Select).find((s) => s.props('ariaLabel') === t('mixer.monitorOf', { input: 'Mic' }))
    monitor!.vm.$emit('update:modelValue', 'monitor_only')
    await flushPromises()
    expect(put.mock.calls.filter((c) => c[0] === '/api/admin/obs/input').map((c) => c[1].body)).toEqual([
      { input: 'Mic', muted: true },
      { input: 'Game audio', volume_db: -12 },
      { input: 'Game audio', volume_db: -100 },
      { input: 'Mic', monitor: 'monitor_only' },
    ])
  })
})
