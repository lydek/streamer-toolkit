import { invoke } from '@tauri-apps/api/core'
import createClient from 'openapi-fetch'
import type { components, paths } from '../api/schema'

export type Schemas = components['schemas']

// Fetched once per page load over IPC and kept only in memory (never localStorage).
let token: Promise<string> | undefined

export class ApiError extends Error {
  constructor(public code: string) {
    super(code)
  }
}

/** Typed admin API client; paths, bodies and responses come from the Host's OpenAPI document. */
export const client = createClient<paths>()

client.use({
  async onRequest({ request }) {
    // A failed fetch is not cached, so the next call asks again.
    token ??= invoke<string>('get_admin_token').catch((e) => {
      token = undefined
      throw e
    })
    request.headers.set('Authorization', `Bearer ${await token}`)
    return request
  },
})

/** Unwraps a client call: resolves to the response data, or throws ApiError with the Host's error code. */
export async function api<T>(call: Promise<{ data?: T; error?: unknown; response: Response }>): Promise<T> {
  const { data, error, response } = await call
  if (!response.ok) {
    const code = (error as { code?: unknown } | undefined)?.code
    throw new ApiError(typeof code === 'string' ? code : `http.${response.status}`)
  }
  return data as T
}
