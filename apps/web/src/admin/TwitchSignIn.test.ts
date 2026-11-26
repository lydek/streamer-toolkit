// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import type { Schemas } from './api'
import TwitchSignIn from './TwitchSignIn.vue'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const { client } = vi.hoisted(() => ({ client: { PUT: vi.fn(), POST: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))

const ok = () => ({ data: undefined, response: new Response(null, { status: 204 }) })
const t = i18n.global.t
const signedIn: Schemas['Overview'] = {
  client_id: 'cid',
  status: { state: 'signed_in', user_id: '42', login: 'streamer' },
  eventsub_connected: true,
  irc_connected: false, scopes: [],
}
const signedOut: Schemas['Overview'] = { client_id: 'cid', status: { state: 'signed_out' }, eventsub_connected: false, irc_connected: false, scopes: [] }
const pending: Schemas['Overview'] = {
  client_id: 'cid',
  eventsub_connected: false,
  irc_connected: false, scopes: [],
  status: { state: 'pending', user_code: 'ABCD', verification_uri: 'https://www.twitch.tv/activate' },
}

/** Mounted like its parents do: `run` performs the action, then the overview changes to `after`. */
function mountSignIn(overview: Schemas['Overview'], after: Schemas['Overview']) {
  const wrapper = mount(TwitchSignIn, {
    props: {
      overview,
      busy: false,
      run: async (action: () => Promise<unknown>) => {
        await action()
        await wrapper.setProps({ overview: after })
      },
    },
    global: { plugins: [i18n, PrimeVue] },
    attachTo: document.body,
  })
  return wrapper
}

async function press(wrapper: ReturnType<typeof mountSignIn>, label: string) {
  const button = wrapper.findAll('button').find((b) => b.text() === label)!
  ;(button.element as HTMLButtonElement).focus()
  await button.trigger('click')
  await flushPromises()
}

afterEach(() => (document.body.innerHTML = ''))

// Each action replaces the button it was started from; focus goes to the main control of the new state.
describe('TwitchSignIn focus', () => {
  it('after signing out, is on sign in', async () => {
    client.POST.mockResolvedValue(ok())
    const wrapper = mountSignIn(signedIn, signedOut)
    await press(wrapper, t('twitch.logout'))
    expect(document.activeElement?.textContent).toBe(t('twitch.login'))
  })

  it('after starting a sign-in, is on opening the activation page', async () => {
    client.POST.mockResolvedValue(ok())
    const wrapper = mountSignIn(signedOut, pending)
    await press(wrapper, t('twitch.login'))
    expect(document.activeElement?.textContent).toBe(t('twitch.openActivate'))
  })

  it('after saving the Client ID, is on sign in', async () => {
    client.PUT.mockResolvedValue(ok())
    const wrapper = mountSignIn(signedOut, signedOut)
    await press(wrapper, t('twitch.changeClientId'))
    expect(document.activeElement?.id).toBe('client-id')
    await wrapper.get('#client-id').setValue('newcid')
    ;(wrapper.get('button[type="submit"]').element as HTMLButtonElement).focus()
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(document.activeElement?.textContent).toBe(t('twitch.login'))
  })
})
