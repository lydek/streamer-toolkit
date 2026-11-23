import { describe, expect, it } from 'vitest'
import { summaryLight } from './services'

const on = { state: 'connected' }
const off = { state: 'error' }

describe('the summary connection light', () => {
  it('stands for the services in use, once all of them are connected', () => {
    expect(summaryLight([], on, on)).toEqual({ service: 'twitch', light: on })
    expect(summaryLight([], on, off)).toBeUndefined()
    // Twitch not used (and not signed in): OBS alone decides, and the light is OBS's.
    expect(summaryLight(['twitch'], off, on)).toEqual({ service: 'obs', light: on })
    expect(summaryLight(['obs'], on, off)).toEqual({ service: 'twitch', light: on })
    expect(summaryLight(['twitch', 'obs'], on, on)).toBeUndefined()
  })
})
