import { ref } from 'vue'
import { api, client } from './api'

/** The text size range in percent (the Host checks the same). */
export const TEXT_SIZE = { min: 90, max: 150, step: 10 } as const

// The saved text size (settings.json); 100 until loaded or when never changed.
const size = ref(100)
let loading: Promise<void> | undefined

export function useTextSize() {
  loading ??= api(client.GET('/api/admin/text-size'))
    .then((saved) => {
      if (saved) size.value = saved
    })
    .catch(() => {
      loading = undefined
    })
  return size
}

/** Scales every rem-sized thing in the admin window. */
export function applyTextSize(percent: number) {
  document.documentElement.style.fontSize = percent === 100 ? '' : `${percent}%`
}

/** Saves the size; the window follows once the Host has it. The caller reports a failure. */
export async function saveTextSize(percent: number) {
  await api(client.PUT('/api/admin/text-size', { body: percent }))
  size.value = percent
}
