// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { type Schemas, client } from './api'
import ScreenToolsPage from './ScreenToolsPage.vue'
import WarningsPage from './WarningsPage.vue'
import ObsSourcesPage from './ObsSourcesPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: vi.fn() }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: 200 }) })
let socket: { onmessage?: (e: { data: string }) => void }
let post: ReturnType<typeof vi.fn>, put: ReturnType<typeof vi.fn>

const card: Schemas['BreakPayload'] = {
  shown: false,
  text: '休息一下',
  endText: '馬上回來',
  endsAt: null,
  mode: 'minutes',
  minutes: 10,
  untilTime: '20:00',
}
const warnings: Schemas['WarningsPayload'] = {
  chatOverlay: true,
  twitch: false,
  mic: true,
  micSeconds: 30,
  micInput: 'Mic',
  chatOverlayDown: false,
  twitchDown: false,
  micMutedSince: null,
  micMissing: false,
}
const snapshot = (payload: Schemas['BreakPayload'], w = warnings) => ({
  type: 'snapshot',
  version: 1,
  seq: 1,
  payload: {
    'chat.removed': [],
    'chat.message': [],
    'obs.status': [],
    'obs.state': [],
    'presentation.queue': [],
    'stream.status': [],
    'bgm.player': [],
    'counter.values': [],
    'counter.marathon': [],
    'screen.break': [{ id: 'b', type: 'screen.break', version: 1, seq: 1, occurredAt: '', source: 'system', payload }],
    'screen.warnings': [{ id: 'w', type: 'screen.warnings', version: 1, seq: 1, occurredAt: '', source: 'system', payload: w }],
    'screen.appearance': [],
    'leaderboard.update': [],
    'chat.text_size': [],
  },
})

beforeEach(() => {
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
  post = vi.fn(() => ok(card))
  put = vi.fn((_: string, init: { body: Schemas['BreakSettingsBody'] }) => ok(init.body))
  vi.spyOn(client, 'POST').mockImplementation(post as never)
  vi.spyOn(client, 'PUT').mockImplementation(put as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

async function page(payload = card, w = warnings, component: typeof ScreenToolsPage | typeof WarningsPage | typeof ObsSourcesPage = ScreenToolsPage) {
  const wrapper = mount(component, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  socket.onmessage!({ data: JSON.stringify(snapshot(payload, w)) })
  await flushPromises()
  return wrapper
}

it('saves the text with the other settings and shows the card for its minutes', async () => {
  const wrapper = await page()
  expect(wrapper.text()).toContain(t('screenTools.break.hidden'))
  const text = wrapper.get('#break-text')
  await text.setValue('準備中')
  await text.trigger('blur')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/break', {
    body: { text: '準備中', end_text: '馬上回來', mode: 'minutes', minutes: 10, until_time: '20:00' },
  })

  await wrapper.findAll('button').find((b) => b.text() === t('screenTools.break.show'))!.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/screen-tools/break/show', { body: { minutes: 10 } })
})

it('counts to the next time of day in the until mode, and hides a shown card', async () => {
  const wrapper = await page({ ...card, mode: 'until', shown: true, endsAt: new Date(Date.now() + 60_000).toISOString() })
  expect(wrapper.text()).toContain(t('screenTools.break.shown'))
  expect(wrapper.find('#break-until').exists()).toBe(true)

  await wrapper.findAll('button').find((b) => b.text() === t('screenTools.break.show'))!.trigger('click')
  await flushPromises()
  const body = post.mock.calls[0][1].body as { until: string }
  const until = new Date(body.until)
  expect([until.getHours(), until.getMinutes()]).toEqual([20, 0])
  expect(until.getTime()).toBeGreaterThan(Date.now())

  await wrapper.findAll('button').find((b) => b.text() === t('screenTools.break.hide'))!.trigger('click')
  await flushPromises()
  expect(post).toHaveBeenCalledWith('/api/admin/screen-tools/break/hide')
})

// 警語 has its own page.
it('saves a warning switch with the other warning settings, and asks for a new mic once it is gone', async () => {
  const wrapper = await page(card, warnings, WarningsPage)
  expect(wrapper.text()).not.toContain(t('screenTools.warnings.inputMissing', { name: 'Mic' }))
  await wrapper.get('input[aria-labelledby="warn-twitch"]').trigger('change')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/warnings', {
    body: { chat_overlay: true, twitch: true, mic: true, mic_seconds: 30, mic_input: 'Mic' },
  })

  const gone = await page(card, { ...warnings, micMissing: true }, WarningsPage)
  expect(gone.text()).toContain(t('screenTools.warnings.inputMissing', { name: 'Mic' }))
})

// The overlays' look is at the bottom of the OBS sources page.
it('saves the overlay theme with the text size', async () => {
  const wrapper = await page(card, warnings, ObsSourcesPage)
  socket.onmessage!({
    data: JSON.stringify({ id: 'a', type: 'screen.appearance', version: 1, seq: 2, occurredAt: '', source: 'system', payload: { theme: 'dark', textSize: 120 } }),
  })
  await flushPromises()
  await wrapper.findAll('[aria-labelledby="overlay-theme-label"] button').find((b) => b.text() === t('screenTools.appearance.themes.cream'))!.trigger('click')
  await flushPromises()
  expect(put).toHaveBeenCalledWith('/api/admin/screen-tools/appearance', { body: { theme: 'cream', textSize: 120 } })
})
