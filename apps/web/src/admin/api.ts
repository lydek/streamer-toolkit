import { invoke } from '@tauri-apps/api/core'

// Fetched once per page load over IPC and kept only in memory (never localStorage).
let token: Promise<string> | undefined

export class ApiError extends Error {
  constructor(public code: string) {
    super(code)
  }
}

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  // A failed fetch is not cached, so the next call asks again.
  token ??= invoke<string>('get_admin_token').catch((e) => {
    token = undefined
    throw e
  })
  const res = await fetch(path, {
    ...init,
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${await token}`, ...init.headers },
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new ApiError(body.code ?? `http.${res.status}`)
  }
  return res.status === 204 ? (undefined as T) : res.json()
}
