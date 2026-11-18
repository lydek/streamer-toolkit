import { reactive } from 'vue'
import { type Schemas, api, client } from './api'

/**
 * The tab bar the user arranged (settings.json): pinned group IDs in order (`null`: never edited, every group
 * pinned; `[]`: none, all in "More") and whether the bar sits on top or at the bottom. Below 600px the bar is
 * always at the bottom with the first four pinned groups.
 */
const state = reactive<{ groups: string[] | null; position: Schemas['TabBarPosition'] }>({ groups: null, position: 'top' })
let loading: Promise<void> | undefined

export function useTabBar() {
  loading ??= api(client.GET('/api/admin/tab-bar'))
    .then((saved) => {
      if (saved) Object.assign(state, saved)
    })
    .catch(() => {
      loading = undefined
    })
  return state
}

/** Saves the arrangement (`null` restores the defaults) and shows it once the Host has it. */
export async function saveTabBar(next: Schemas['TabBar'] | null) {
  await api(client.PUT('/api/admin/tab-bar', { body: next }))
  Object.assign(state, next ?? { groups: null, position: 'top' })
}
