import { describe, expect, it } from 'vitest'
import { FOLLOW_PREVIEW, mixScene, mixSections } from './mixer'

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
})
