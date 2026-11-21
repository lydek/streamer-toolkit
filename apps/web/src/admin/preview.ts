import { computed, shallowRef } from 'vue'
import { routeToOutput } from './outputDevice'

/** The one sound or track being previewed in the admin window; starting another stops it. */
const current = shallowRef<{ url: string; audio: HTMLAudioElement }>()

/** The URL being previewed now, if any. */
export const previewUrl = computed(() => current.value?.url)

/** Stops the preview of `url` (or whatever plays, without one). */
export function stopPreview(url?: string) {
  if (!current.value || (url && current.value.url !== url)) return
  current.value.audio.pause()
  current.value = undefined
}

/** Starts previewing `url` at `volume` (0–100), stopping any other preview; a second press on it stops it. */
export function togglePreview(url: string, volume = 100) {
  if (current.value?.url === url) return stopPreview()
  stopPreview()
  const audio = new Audio(url)
  audio.volume = Math.min(Math.max(volume, 0), 100) / 100
  const done = () => stopPreview(url)
  audio.onended = audio.onerror = done
  current.value = { url, audio }
  // On the chosen output device; stopped meanwhile, it never starts.
  void routeToOutput(audio).then(() => {
    if (current.value?.audio === audio) audio.play()?.catch(done)
  })
}
