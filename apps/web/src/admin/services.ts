import { reactive } from 'vue'
import { type Schemas, api, client } from './api'

export type Service = Schemas['ExternalService']
export type ServiceUse = Schemas['ServiceUse']

/**
 * Twitch or OBS not used at all (settings.json): `unused` lists them, `parked` the modules that were on and went off
 * for it, so turning the service back on turns exactly those on again. Both in use until the saved choice loads.
 */
const state = reactive<ServiceUse>({ unused: [], parked: [] })
let loading: Promise<void> | undefined

export function useServices() {
  loading ??= api(client.GET('/api/admin/services'))
    .then((saved) => {
      if (saved) Object.assign(state, saved)
    })
    .catch(() => {
      loading = undefined
    })
  return state
}

/** Saves the choice and shows it once the Host has it. */
export async function saveServices(next: ServiceUse) {
  await api(client.PUT('/api/admin/services', { body: next }))
  Object.assign(state, next)
}

/** The one light standing for all connections: the light of a service in use, once every one in use is connected;
 * none while one is not, or when neither is used. */
export function summaryLight<L extends { state: string }>(unused: Service[], twitch: L, obs: L) {
  const used = ([['twitch', twitch], ['obs', obs]] as const).filter(([s]) => !unused.includes(s))
  if (!used.length || used.some(([, l]) => l.state !== 'connected')) return undefined
  return { service: used[0][0], light: used[0][1] }
}
