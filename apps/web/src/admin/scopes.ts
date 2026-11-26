import type { Router } from 'vue-router'
import { api, client } from './api'
import type { Feature } from './features'
import { twitchOverview } from './home'

/**
 * Twitch scopes per feature (docs/decisions.md Twitch scope): asked for the first time a feature needs them, then at
 * every sign-in. The Host keeps them and refuses any that are not a feature's.
 */

/** The scopes of `f` the current sign-in has not granted. */
export const missingScopes = (f: Feature) => {
  const granted = new Set(twitchOverview.value?.scopes ?? [])
  return (f.scopes ?? []).filter((s) => !granted.has(s))
}

/** Signs in again asking for `scopes` too, then opens Settings → Connections, where the code to enter waits. */
export async function requestScopes(scopes: string[], router: Router) {
  await api(client.POST('/api/admin/twitch/login', { body: { add_scopes: scopes } }))
  await router.push({ path: '/settings/connections', hash: '#twitch' })
}
