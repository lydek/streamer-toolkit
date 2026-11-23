// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import { client } from './api'
import { FOLLOW_PREVIEW, meterFill, mixScene, mixSections, useMeters } from './mixer'

const input = (name: string, scenes: string[] = [], global = false) => ({ name, muted: false, scenes, global })
const state = {
  scene: 'Game',
  preview: 'Chat',
  studioMode: true,
  scenes: [{ name: 'Game', items: [] }, { name: 'Chat', items: [] }],
  inputs: [input('Mic', [], true), input('Game audio', ['Game']), input('Discord', ['Chat'])],
  streaming: false,
  recording: false,
  transition: null,
}

describe('the mixer', () => {
  it('follows the live scene, the preview in studio mode, or a picked scene while it exists', () => {
    expect(mixScene(state, '')).toBe('Game')
    expect(mixScene(state, FOLLOW_PREVIEW)).toBe('Chat')
    expect(mixScene({ ...state, studioMode: false, preview: null }, FOLLOW_PREVIEW)).toBe('Game')
    expect(mixScene(state, 'Chat')).toBe('Chat')
    expect(mixScene(state, 'Removed')).toBe('Game')
  })

  it('lists global inputs in every scene, and each input once', () => {
    expect(mixSections(state, 'Game')).toEqual({ on: [state.inputs[0], state.inputs[1]], off: [state.inputs[2]] })
    expect(mixSections(state, 'Chat').on.map((i) => i.name)).toEqual(['Mic', 'Discord'])
    expect(mixSections(state, undefined).on.map((i) => i.name)).toEqual(['Mic'])
  })

  it('fills the meter from the fader floor to 0 dB, and not while muted', () => {
    expect(meterFill(-30, false)).toBe(50)
    expect(meterFill(-100, false)).toBe(0)
    expect(meterFill(3, false)).toBe(100)
    expect(meterFill(-6, true)).toBe(0)
    expect(meterFill(undefined, false)).toBe(0)
  })

  it('polls the meters in one loop, even switched off and on while a request is out', async () => {
    let answer!: () => void
    const pending = () =>
      new Promise((r) => {
        answer = () => r({ data: [], response: new Response(null, { status: 200 }) })
      })
    const get = vi.spyOn(client, 'GET').mockImplementation(pending as never)
    const active = ref(true)
    const scope = effectScope()
    scope.run(() => useMeters(active))
    active.value = false
    await nextTick()
    active.value = true
    await nextTick()
    expect(get).toHaveBeenCalledTimes(1)
    answer()
    scope.stop()
    get.mockRestore()
  })
})
