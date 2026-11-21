import { readFileSync } from 'node:fs'
import type { Locator, Page, Route } from '@playwright/test'
import type { components } from '../src/api/schema'

type Schemas = components['schemas']

/** UI strings, so tests find controls by the text users see. */
export const zh = JSON.parse(readFileSync(new URL('../src/locales/zh-TW.json', import.meta.url), 'utf8'))

/** What the mocked admin API answers; tests change it to change later answers. */
export interface AdminState {
  /** `eventsub_connected` defaults to whether Twitch is signed in, `irc_connected` to false. */
  overview: Omit<Schemas['Overview'], 'eventsub_connected' | 'irc_connected'> & { eventsub_connected?: boolean; irc_connected?: boolean }
  modules: Schemas['ModuleState'][]
  template: Schemas['TemplateStatus']
  alertTemplate: Schemas['AlertTemplateChoice']
  general: Schemas['GeneralSettings']
  obs: Schemas['ObsOverview']
  /** Saved home tile order; `null` = the defaults. PUT stores it. */
  homeTiles: string[] | null
  /** PUT stores it and answers it back. */
  checkin: Schemas['CheckinSettingsBody']
  /** The new-version notice: none by default. */
  update: Schemas['UpdateState']
  /** This stream's numbers; `null` = no stream yet. */
  streamSummary: Schemas['StreamSummary'] | null
}

export interface Invoke {
  cmd: string
  args: Record<string, unknown>
}

declare global {
  interface Window {
    __e2eInvokes: Invoke[]
    __e2eEmit: (event: string, payload: unknown) => void
  }
}

/**
 * Fakes the Tauri IPC the admin window uses: records every `invoke`, answers `get_admin_token` with a dummy, and
 * lets `emit` deliver events to `listen` handlers (the parts of @tauri-apps/api/mocks this needs, as page script).
 */
export async function mockIpc(page: Page) {
  await page.addInitScript(() => {
    const callbacks = new Map<number, (payload: unknown) => void>()
    const listeners: Record<string, number[]> = {}
    let nextId = 1
    window.__e2eInvokes = []
    window.__e2eEmit = (event, payload) => {
      for (const id of listeners[event] ?? []) callbacks.get(id)?.({ event, id, payload })
    }
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        transformCallback(callback: (payload: unknown) => void, once = false) {
          const id = nextId++
          callbacks.set(id, (payload) => {
            if (once) callbacks.delete(id)
            callback(payload)
          })
          return id
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        async invoke(cmd: string, args: Record<string, unknown> = {}) {
          window.__e2eInvokes.push({ cmd, args })
          if (cmd === 'plugin:event|listen') {
            ;(listeners[args.event as string] ??= []).push(args.handler as number)
            return args.handler
          }
          // Not a secret: the mocked API below accepts anything.
          if (cmd === 'get_admin_token') return 'e2e-dummy-token'
          return null
        },
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
    })
  })
}

/** The IPC calls made so far, optionally only those of one command. */
export const invokes = (page: Page, cmd?: string) =>
  page.evaluate((cmd) => window.__e2eInvokes.filter((i) => !cmd || i.cmd === cmd), cmd)

/** Delivers a Tauri event (as Rust's `emit` would) to the page's listeners. */
export const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(([event, payload]) => window.__e2eEmit(event as string, payload), [event, payload])

const hostPath = (url: URL) => /^\/(api|ws|templates|sounds|music)(\/|$)/.test(url.pathname)

/**
 * Mocks the IPC, the admin API and `/ws/overlay` for the admin UI on the Vite dev server. Requests to Host paths
 * that no test mocks get a 404 instead of reaching the dev proxy (and a Host that may be running). Later
 * `page.route` calls take precedence, so a test adds or overrides endpoints after this.
 */
export async function mockAdmin(page: Page, state: Partial<AdminState> = {}): Promise<AdminState> {
  const s: AdminState = {
    overview: { client_id: '', status: { state: 'signed_out' } },
    modules: [{ id: 'chat', enabled: true, closed: false }],
    template: { selected: 'default', error: null },
    alertTemplate: { selected: 'default' },
    general: { ask_before_close: true },
    obs: { host: '127.0.0.1', port: 4455, password_set: false, status: { state: 'reconnecting', reason: null } },
    homeTiles: null,
    checkin: { command: '打卡', time_zone: 'Asia/Taipei', reset_hour: 0, card_slots: 10, reward_id: null },
    update: { current: '0.1.0', available: null, check: true },
    streamSummary: null,
    ...state,
  }
  await mockIpc(page)
  await page.route(hostPath, (r) => r.fulfill({ status: 404, json: { code: 'e2e.unmocked' } }))
  const get = (pick: () => unknown) => (r: Route) =>
    r.request().method() === 'GET' ? r.fulfill({ json: pick() }) : r.fallback()
  await page.route(
    '**/api/admin/twitch',
    get(() => ({ eventsub_connected: s.overview.status.state === 'signed_in', irc_connected: false, ...s.overview })),
  )
  await page.route('**/api/admin/modules', get(() => s.modules))
  await page.route('**/api/admin/templates/chat', (r) => {
    if (r.request().method() !== 'PUT') return r.fulfill({ json: s.template })
    s.template = { ...s.template, ...r.request().postDataJSON() }
    return r.fulfill({ status: 204 })
  })
  await page.route('**/api/admin/templates/alerts', (r) => {
    if (r.request().method() !== 'PUT') return r.fulfill({ json: s.alertTemplate })
    s.alertTemplate = r.request().postDataJSON()
    return r.fulfill({ status: 204 })
  })
  await page.route('**/api/admin/general', get(() => s.general))
  await page.route('**/api/admin/obs', get(() => s.obs))
  await page.route('**/api/admin/update', get(() => s.update))
  await page.route('**/api/admin/stream/summary', get(() => s.streamSummary))
  await page.route('**/api/admin/alerts/recent', get(() => []))
  await page.route('**/api/admin/chat/messages', get(() => []))
  await page.route('**/api/admin/chat-display', (r) => (r.request().method() === 'GET' ? r.fulfill({ json: null }) : r.fulfill({ status: 204 })))
  await page.route('**/api/admin/chat/badges', get(() => []))
  await page.route('**/api/admin/viewers/avatars**', get(() => ({})))
  await page.route('**/api/admin/home-tiles', (r) => {
    if (r.request().method() !== 'PUT') return r.fulfill({ json: s.homeTiles })
    s.homeTiles = r.request().postDataJSON()
    return r.fulfill({ status: 204 })
  })
  await page.route('**/api/admin/checkin/rewards', get(() => []))
  await page.route('**/api/admin/sounds', get(() => ({ sounds: [], slots: {} })))
  await page.route('**/api/admin/checkin/outbox', get(() => ({ pending: 0, oldest_pending_at: null, dead_letters: [] })))
  await page.route('**/api/admin/checkin/settings', (r) => {
    if (r.request().method() === 'PUT') s.checkin = r.request().postDataJSON()
    return r.fulfill({ json: s.checkin })
  })
  const snapshot: Schemas['Snapshot'] = {
    type: 'snapshot',
    version: 1,
    seq: 0,
    payload: { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [] },
  }
  await page.routeWebSocket(hostPath, (ws) => ws.send(JSON.stringify(snapshot)))
  return s
}

/** Opens the admin home with mocks in place and waits until its tiles have loaded. */
export async function openAdmin(page: Page, state: Partial<AdminState> = {}) {
  const s = await mockAdmin(page, state)
  await page.goto('/admin/')
  await page.locator('[data-drawer-tile], [data-drawer-arrange]').first().waitFor()
  return s
}

/**
 * True when the page, or the given element, scrolls sideways (WCAG 1.4.10 reflow). Check dialogs and drawers
 * themselves: content in the top layer never widens the page.
 */
export const scrollsSideways = (target: Page | Locator) =>
  'goto' in target
    ? target.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)
    : target.evaluate((el) => el.scrollWidth > el.clientWidth)
