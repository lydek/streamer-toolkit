import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

// The client sends page-relative paths; Node has no page, so resolve them against the Host.
class PageRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(typeof input === 'string' ? new URL(input, 'http://127.0.0.1:47810') : input, init)
  }
}

let fetchMock: ReturnType<typeof vi.fn>

/** Loads a fresh copy of the module (no cached token) whose fetch answers with `responses` in order. */
async function load(...responses: Response[]) {
  vi.resetModules()
  fetchMock = vi.fn(async () => responses.shift())
  vi.stubGlobal('fetch', fetchMock)
  vi.stubGlobal('Request', PageRequest)
  return import('./api')
}

beforeEach(() => invoke.mockReset())
afterEach(() => vi.unstubAllGlobals())

describe('api', () => {
  it('fetches the token once and sends it as a bearer token', async () => {
    invoke.mockResolvedValue('secret')
    const overview = { client_id: '', status: { state: 'signed_out' } }
    const { api, client } = await load(Response.json(overview), new Response(null, { status: 204 }))

    expect(await api(client.GET('/api/admin/twitch'))).toEqual(overview)
    expect(await api(client.POST('/api/admin/twitch/logout'))).toBeUndefined()
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('get_admin_token')
    for (const [req] of fetchMock.mock.calls) expect((req as Request).headers.get('Authorization')).toBe('Bearer secret')
  })

  it('asks for the token again after a failed fetch', async () => {
    invoke.mockRejectedValueOnce(new Error('ipc down')).mockResolvedValue('secret')
    const { api, client } = await load(new Response(null, { status: 204 }))

    await expect(api(client.POST('/api/admin/twitch/logout'))).rejects.toThrow('ipc down')
    await expect(api(client.POST('/api/admin/twitch/logout'))).resolves.toBeUndefined()
    expect(invoke).toHaveBeenCalledTimes(2)
  })

  it('throws the Host error code, or http.<status> without one', async () => {
    invoke.mockResolvedValue('secret')
    const { ApiError, api, client } = await load(
      Response.json({ code: 'simulate.invalid_text' }, { status: 400 }),
      new Response(null, { status: 401 }),
      new Response('Failed to parse the request body as JSON', { status: 400 }),
    )
    const code = () =>
      api(client.POST('/api/admin/simulate/chat', { body: { text: '' } })).then(
        () => 'resolved',
        (e) => (e instanceof ApiError ? e.code : e),
      )

    expect(await code()).toBe('simulate.invalid_text')
    expect(await code()).toBe('http.401')
    expect(await code()).toBe('http.400')
  })
})
