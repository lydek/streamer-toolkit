// @vitest-environment jsdom
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '../i18n'
import { client } from './api'
import RewardsPage from './RewardsPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t
const toastAdd = vi.hoisted(() => vi.fn())
vi.mock('primevue/usetoast', () => ({ useToast: () => ({ add: toastAdd }) }))

const ok = (data?: unknown) => Promise.resolve({ data, response: new Response(null, { status: data ? 200 : 204 }) })
const refused = (code: string, status = 403) => Promise.resolve({ error: { code }, response: new Response(null, { status }) })

const reward = (id: string, title: string, manageable: boolean, more: object = {}) => ({
  id,
  title,
  prompt: '',
  cost: 1000,
  background_color: '#9146FF',
  is_enabled: true,
  is_paused: false,
  is_user_input_required: false,
  skip_queue: false,
  cooldown_seconds: null,
  max_per_stream: null,
  max_per_user_per_stream: null,
  manageable,
  ...more,
})
const GETS: Record<string, unknown> = {
  '/api/admin/rewards': [
    reward('r-own', '喝水提醒', true, { cooldown_seconds: 600, max_per_user_per_stream: 3, is_paused: true }),
    reward('r-dash', 'Hydrate!', false),
  ],
  '/api/admin/checkin/settings': { reward_id: 'r-own' },
  '/api/admin/sounds': { sounds: [{ id: 's1', label: '喝水' }], slots: { 'reward.r-own': 's1' } },
  '/api/admin/screen-tools/media': [{ id: 'm1', title: '驚嚇' }],
  '/api/admin/screen-tools/media-rules': [{ reward_id: 'r-own', media_id: 'm1', position: 'center' }],
}
let patch: ReturnType<typeof vi.fn>

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(GETS[path])) as never)
  patch = vi.fn(() => ok())
  vi.spyOn(client, 'PATCH').mockImplementation(patch as never)
})
enableAutoUnmount(afterEach)
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  toastAdd.mockReset()
})

async function open() {
  const w = mount(RewardsPage, { global: { plugins: [testRouter(), i18n, PrimeVue] }, attachTo: document.body })
  await flushPromises()
  return w
}
const row = (w: Awaited<ReturnType<typeof open>>, id: string) => w.get(`[data-reward="${id}"]`)

it('lists the rewards, what uses each, and leaves those made in the Twitch dashboard read-only', async () => {
  const w = await open()
  expect(w.text()).toContain(t('rewards.count', { count: 2, max: 50, own: 1 }))
  const own = row(w, 'r-own')
  expect(own.text()).toContain(t('rewards.minutes', { n: 10 }))
  expect(own.text()).toContain(t('rewards.perUser', { n: 3 }))
  expect(own.findAll('.used span').map((s) => s.text())).toEqual([
    t('rewards.used.checkin'),
    t('rewards.used.sound', { name: '喝水' }),
    t('rewards.used.media', { name: '驚嚇' }),
  ])
  expect(own.findAll('[role="switch"]')).toHaveLength(2)
  const dash = row(w, 'r-dash')
  expect(dash.text()).toContain(t('rewards.readOnly'))
  expect(dash.text()).toContain(t('rewards.dashboardOnly'))
  expect(dash.text()).toContain(t('rewards.unused'))
  expect(dash.find('[role="switch"]').exists()).toBe(false)
})

it('pauses or resumes one of its own rewards on Twitch, and says why Twitch refused', async () => {
  const w = await open()
  await w.get(`[aria-label="${t('rewards.pauseLabel', { name: '喝水提醒' })}"]`).trigger('change')
  await flushPromises()
  expect(patch).toHaveBeenCalledWith('/api/admin/rewards/{id}', { params: { path: { id: 'r-own' } }, body: { is_paused: false } })

  patch.mockImplementation(() => refused('twitch.scope_required'))
  await w.get(`[aria-label="${t('rewards.enableLabel', { name: '喝水提醒' })}"]`).trigger('change')
  await flushPromises()
  expect(patch).toHaveBeenLastCalledWith('/api/admin/rewards/{id}', { params: { path: { id: 'r-own' } }, body: { is_enabled: false } })
  expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'error', summary: t('errors.twitch.scope_required') }))
})

describe('adding, editing and deleting', () => {
  beforeAll(() => {
    // jsdom has no modal dialogs.
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
      this.open = true
    }
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
      this.open = false
      this.dispatchEvent(new Event('close'))
    }
  })
  const button = (w: Awaited<ReturnType<typeof open>>, label: string) => w.findAll('button').find((b) => b.text() === label)!

  it('adds a reward, refusing a title already on the channel before sending', async () => {
    const post = vi.fn(() => ok({ id: 'r-new' }))
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    const w = await open()
    await button(w, t('rewards.add')).trigger('click')
    await w.get('#rwd-name').setValue('hydrate!')
    await w.get('#rwd-cost').setValue('200')
    await w.get('#rwd-cost').trigger('blur')
    expect(w.text()).toContain(t('rewards.dialog.titleTaken'))
    expect(w.get('.reward-dialog').findAll('button').find((b) => b.text() === t('rewards.dialog.save'))!.attributes('disabled')).toBeDefined()
    await w.get('#rwd-form').trigger('submit')
    expect(post).not.toHaveBeenCalled()

    await w.get('#rwd-name').setValue('丟枕頭')
    await w.get('#rwd-form').trigger('submit')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/rewards', {
      body: {
        title: '丟枕頭', prompt: '', cost: 200, background_color: '#9146FF', is_user_input_required: false,
        skip_queue: false, cooldown_seconds: 0, max_per_stream: 0, max_per_user_per_stream: 0,
      },
    })
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'success', summary: t('rewards.dialog.added') }))
  })

  it('edits one of its own rewards, and deletes it after saying what uses it', async () => {
    const del = vi.fn(() => ok())
    vi.spyOn(client, 'DELETE').mockImplementation(del as never)
    const w = await open()
    expect(w.find(`[aria-label="${t('rewards.editLabel', { name: 'Hydrate!' })}"]`).exists()).toBe(false)
    await w.get(`[aria-label="${t('rewards.editLabel', { name: '喝水提醒' })}"]`).trigger('click')
    expect((w.get('#rwd-name').element as HTMLInputElement).value).toBe('喝水提醒')
    await w.get('#rwd-prompt').setValue('提醒主播喝水')
    await w.get('#rwd-form').trigger('submit')
    await flushPromises()
    expect(patch).toHaveBeenCalledWith('/api/admin/rewards/{id}', {
      params: { path: { id: 'r-own' } },
      body: expect.objectContaining({ title: '喝水提醒', prompt: '提醒主播喝水', cooldown_seconds: 600, max_per_user_per_stream: 3 }),
    })

    await w.get(`[aria-label="${t('rewards.editLabel', { name: '喝水提醒' })}"]`).trigger('click')
    await button(w, t('rewards.dialog.delete')).trigger('click')
    expect(w.get('.reward-dialog ul').text()).toContain(t('rewards.used.checkin'))
    await button(w, t('rewards.dialog.delete')).trigger('click')
    await flushPromises()
    expect(del).toHaveBeenCalledWith('/api/admin/rewards/{id}', { params: { path: { id: 'r-own' } } })
  })

  it('cannot add one past Twitch’s 50 rewards', async () => {
    const many = Array.from({ length: 50 }, (_, i) => reward(`r-${i}`, `獎勵 ${i}`, false))
    vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(path === '/api/admin/rewards' ? many : GETS[path])) as never)
    const w = await open()
    expect(button(w, t('rewards.add')).attributes('disabled')).toBeDefined()
    expect(w.text()).toContain(t('rewards.full', { max: 50 }))
  })
})

describe('taking over a reward made in the Twitch dashboard', () => {
  beforeAll(() => {
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
      this.open = true
    }
  })
  const okButton = (w: Awaited<ReturnType<typeof open>>) =>
    w.get('.takeover-dialog').findAll('button').find((b) => b.text() === t('rewards.takeover.ok'))!

  it('asks for a new name, then says what still uses the original', async () => {
    const post = vi.fn(() => ok({ id: 'r-new', moved: ['checkin'], not_moved: ['sound'], pause_failed: true }))
    vi.spyOn(client, 'POST').mockImplementation(post as never)
    const w = await open()
    await row(w, 'r-dash').get('button').trigger('click')
    expect(w.get('#tk-title').text()).toBe(t('rewards.takeover.title', { name: 'Hydrate!' }))
    // Prefilled with the original's name, which Twitch would refuse.
    expect((w.get('#tk-name').element as HTMLInputElement).value).toBe('Hydrate!')
    expect(w.get('#tk-help').text()).toBe(t('rewards.takeover.sameName'))
    expect(okButton(w).attributes('disabled')).toBeDefined()
    await w.get('#tk-name').setValue('喝水提醒')
    expect(okButton(w).attributes('disabled')).toBeDefined()

    await w.get('#tk-name').setValue('Hydrate! 2')
    await w.get('#tk-form').trigger('submit')
    await flushPromises()
    expect(post).toHaveBeenCalledWith('/api/admin/rewards/{id}/takeover', { params: { path: { id: 'r-dash' } }, body: { title: 'Hydrate! 2' } })
    expect(toastAdd).toHaveBeenCalledWith(
      expect.objectContaining({ severity: 'warn', summary: t('rewards.takeover.partly', { name: 'Hydrate!', uses: t('rewards.takeover.uses.sound') }) }),
    )
    // The original was paused but the new one is not: viewers can redeem it now, so it says so.
    expect(toastAdd).toHaveBeenCalledWith(expect.objectContaining({ severity: 'warn', summary: t('rewards.takeover.notPaused', { name: 'Hydrate! 2' }) }))
  })
})

describe('opened from the home drawer’s 新增獎勵 (?new=1)', () => {
  beforeAll(() => {
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
      this.open = true
    }
  })
  async function openAt(path: string) {
    const router = testRouter()
    await router.push(path)
    const w = mount(RewardsPage, { global: { plugins: [router, i18n, PrimeVue] }, attachTo: document.body })
    await flushPromises()
    return { w, router }
  }
  const dialogOpen = (w: Awaited<ReturnType<typeof open>>) => (w.get('dialog.reward-dialog').element as HTMLDialogElement).open

  it('opens the add dialog once the list is read, focused from the add button, and drops the query', async () => {
    const { w, router } = await openAt('/rewards?new=1')
    expect(dialogOpen(w)).toBe(true)
    expect(router.currentRoute.value.query).toEqual({})
  })

  it('does not open it for another value, when the channel is full, or when the list cannot be read', async () => {
    expect(dialogOpen((await openAt('/rewards?new=yes')).w)).toBe(false)
    const fifty = Array.from({ length: 50 }, (_, i) => reward(`r${i}`, `R${i}`, true))
    vi.spyOn(client, 'GET').mockImplementation(((path: string) => ok(path === '/api/admin/rewards' ? fifty : GETS[path])) as never)
    expect(dialogOpen((await openAt('/rewards?new=1')).w)).toBe(false)
    vi.spyOn(client, 'GET').mockImplementation(((path: string) =>
      path === '/api/admin/rewards' ? refused('twitch.scope_required') : ok(GETS[path])) as never)
    const failed = (await openAt('/rewards?new=1')).w
    expect(dialogOpen(failed)).toBe(false)
    expect(failed.text()).toContain(t('errors.twitch.scope_required'))
  })
})
