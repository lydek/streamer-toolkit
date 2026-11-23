import { ref } from 'vue'
import type { Schemas } from './api'

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
