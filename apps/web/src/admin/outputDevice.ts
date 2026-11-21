import { shallowRef } from 'vue'

/*
 * 試聽用的輸出裝置: the admin window's previews (sounds, music) play on the chosen device; OBS keeps its own. The
 * choice is a local preference of this window, kept in localStorage (no secret). A device that is gone falls back
 * to the default one.
 */
const KEY = 'preview-output-device'

export interface OutputDevice {
  id: string
  label: string
}

function read(): OutputDevice | undefined {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null') as OutputDevice | null
    return saved?.id ? saved : undefined
  } catch {
    return undefined
  }
}

/** The chosen device; `undefined` is the default one. */
export const outputDevice = shallowRef<OutputDevice | undefined>(read())

export function chooseOutput(device: OutputDevice | undefined) {
  outputDevice.value = device
  if (device) localStorage.setItem(KEY, JSON.stringify(device))
  else localStorage.removeItem(KEY)
}

/** Sends `audio` to the chosen device; when it is gone (or the browser cannot choose), the default plays. */
export async function routeToOutput(audio: HTMLAudioElement) {
  const id = outputDevice.value?.id
  // Older engines cannot choose a device at all.
  if (!id || typeof audio.setSinkId !== 'function') return
  try {
    await audio.setSinkId(id)
  } catch {
    await audio.setSinkId('').catch(() => {})
  }
}

/**
 * The output devices, and whether the browser gave names. Names (and, in Chromium, every device but the default)
 * come only after `withNames` asks the microphone question once.
 */
export async function listOutputs(withNames = false): Promise<{ devices: OutputDevice[]; named: boolean }> {
  const media = navigator.mediaDevices
  if (!media?.enumerateDevices) return { devices: [], named: true }
  if (withNames) {
    // Device names need a microphone grant; it is used only for this listing and released at once.
    try {
      const stream = await media.getUserMedia({ audio: true })
      stream.getTracks().forEach((t) => t.stop())
    } catch {
      // Without it the devices still list, unnamed.
    }
  }
  const all = await media.enumerateDevices()
  const devices = all
    .filter((d) => d.kind === 'audiooutput' && d.deviceId && d.deviceId !== 'default' && d.deviceId !== 'communications')
    .map((d, i) => ({ id: d.deviceId, label: d.label || `#${i + 1}` }))
  // Labels come all at once with the grant, so any label means it was given.
  return { devices, named: all.some((d) => d.label) }
}
