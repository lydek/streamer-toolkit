import { api, client } from './api'

// Asked once per app load: home sends a data folder that never finished or skipped the setup to /setup.
let asked: Promise<boolean> | undefined

/** Whether the first-run setup should show. A failed request counts as done, so the app is never stuck there. */
export function needsSetup(): Promise<boolean> {
  asked ??= api(client.GET('/api/admin/first-run')).then(
    (r) => !r.done,
    () => false,
  )
  return asked
}

/** Finished or skipped: it never shows again. */
export async function finishSetup() {
  await api(client.PUT('/api/admin/first-run', { body: { done: true } }))
  asked = Promise.resolve(false)
}

/** Tests only. */
export function forgetSetup() {
  asked = undefined
}
