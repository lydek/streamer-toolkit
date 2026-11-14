import { reactive } from 'vue'

/** Test controls, as `window.e2e`: settings saved so far, whether saves fail, whether the pending item shows. */
export const controls = reactive({ saved: [] as string[], fail: false, pending: true })

export type Controls = typeof controls

Object.assign(window, { e2e: controls })
