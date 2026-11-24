// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import { followDrag } from './drag'

/** Starts a drag on a fresh element; returns it with the `move` and `end` spies. */
function drag() {
  const el = document.createElement('div')
  el.setPointerCapture = vi.fn()
  const move = vi.fn()
  const end = vi.fn()
  el.addEventListener('pointerdown', (e) => followDrag(e as PointerEvent, move, end))
  el.dispatchEvent(Object.assign(new Event('pointerdown'), { pointerId: 1 }))
  return { el, move, end }
}

describe('followDrag', () => {
  it.each(['pointerup', 'pointercancel'])('ends once when the pointer is taken back after %s', (last) => {
    const { el, move, end } = drag()
    expect(el.setPointerCapture).toHaveBeenCalledWith(1)
    el.dispatchEvent(new Event('pointermove'))
    expect(move).toHaveBeenCalledTimes(1)
    el.dispatchEvent(new Event(last))
    el.dispatchEvent(new Event('lostpointercapture'))
    el.dispatchEvent(new Event('lostpointercapture'))
    expect(end).toHaveBeenCalledTimes(1)
    el.dispatchEvent(new Event('pointermove'))
    expect(move).toHaveBeenCalledTimes(1)
  })
})
