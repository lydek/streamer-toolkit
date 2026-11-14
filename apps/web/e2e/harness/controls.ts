import { reactive } from 'vue'

/** Test controls, as `window.e2e`: settings saved so far, whether saves fail, whether the pending item shows. */
export interface Controls {
  saved: string[]
  fail: boolean
  pending: boolean
}

export const controls: Controls = reactive({ saved: [], fail: false, pending: true })

Object.assign(window, { e2e: controls })
