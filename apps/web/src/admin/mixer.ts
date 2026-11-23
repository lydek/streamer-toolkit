import { onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue'
import { type Schemas, api, client } from './api'

type State = Schemas['StreamingState']
type Input = Schemas['Input']

/*
 * 音訊混音: which scene it shows and which inputs that scene has. Volume, mute and monitoring belong to the input
 * (every scene), so a scene only decides where an input is listed.
 */

/** The pick that follows the preview scene in studio mode; '' follows the live scene, anything else is a scene name. */
export const FOLLOW_PREVIEW = '\u0000preview'

/** The mixer's pick, kept while the admin window is open: a picked scene sticks until 「跟著直播中」 is picked again. */
export const mixPick = ref('')

/** The scene shown for `pick`: the live one, unless the picked scene (or the preview, in studio mode) exists. */
export function mixScene(state: State, pick: string): string | undefined {
  if (pick === FOLLOW_PREVIEW) return (state.studioMode && state.preview) || state.scene || undefined
  if (pick && state.scenes.some((s) => s.name === pick)) return pick
  return state.scene ?? undefined
}

/** In the scene: a global audio device, or an input the scene contains (heard or not). */
export const inScene = (input: Input, scene: string | undefined) => !!input.global || (!!scene && !!input.scenes?.includes(scene))

/** The scene's inputs, then the rest; each input once, in OBS's order. */
export function mixSections(state: State, scene: string | undefined) {
  return {
    on: state.inputs.filter((i) => inScene(i, scene)),
    off: state.inputs.filter((i) => !inScene(i, scene)),
  }
}

/** The fader's floor: at or below it the input is silent (OBS's −∞). */
export const FADER_MIN = -60

/** How often the meters are asked while they show; the Host keeps only OBS's latest levels. */
export const METER_MS = 100

/**
 * The inputs' latest peak levels (dB) while `active` and the window shows, asked one request at a time; empty
 * otherwise. A failed request leaves the meters empty until the next one: they are only a picture of the sound.
 */
export function useMeters(active: Ref<boolean>) {
  const meters = shallowRef(new Map<string, number>())
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  // One loop at a time: switched off and on while a request is out, the running loop goes on.
  let running = false
  async function tick() {
    timer = undefined
    if (stopped || !active.value) {
      running = false
      return
    }
    if (!document.hidden) {
      try {
        const got = await api(client.GET('/api/admin/obs/meters'))
        meters.value = new Map(got.map((m) => [m.name, m.peakDb]))
      } catch {
        meters.value = new Map()
      }
    }
    if (!stopped && active.value) timer = setTimeout(tick, METER_MS)
    else running = false
  }
  watch(
    active,
    (on) => {
      if (on && !running) {
        running = true
        void tick()
      }
      if (!on) meters.value = new Map()
    },
    { immediate: true },
  )
  onScopeDispose(() => {
    stopped = true
    clearTimeout(timer)
  })
  return meters
}

// The input chosen as the mic, shared by the page and the home tile; loaded on first use.
const mic = ref<string | null>()
let micLoading: Promise<void> | undefined

/** The input chosen as the mic: `undefined` while loading, `null` when none is chosen (it is never guessed). */
export function useMic() {
  micLoading ??= api(client.GET('/api/admin/mixer/mic'))
    .then((m) => {
      mic.value = m.input ?? null
    })
    .catch(() => {
      micLoading = undefined
    })
  return mic
}

/** Saves the mic choice; shown at once, put back if saving fails (the error is thrown on). */
export async function setMic(input: string | null) {
  const before = mic.value
  mic.value = input
  try {
    await api(client.PUT('/api/admin/mixer/mic', { body: { input } }))
  } catch (e) {
    mic.value = before
    throw e
  }
}

/** The meter's fill, 0-100 (%) from the fader's floor to 0 dB; empty while muted. */
export const meterFill = (db: number | undefined, muted: boolean) =>
  muted || db === undefined ? 0 : Math.round(Math.min(1, Math.max(0, (db - FADER_MIN) / -FADER_MIN)) * 100)
