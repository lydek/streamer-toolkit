import { describe, expect, it } from 'vitest'
import { alignedCorner, boxOf, pointOf } from './relativeAlign'

describe('relative alignment', () => {
  it('puts an item below another, centred, with an offset (the issue example)', () => {
    const ref = { left: 100, top: 200, width: 300, height: 100 }
    expect(alignedCorner({ width: 100, height: 50 }, ref, 'bottom', 'middle', { x: 0, y: 10 }, false)).toEqual({ left: 200, top: 310 })
  })

  it('keeps inside the canvas edges, and overlaps the middle for center', () => {
    const canvas = { left: 0, top: 0, width: 1920, height: 1080 }
    const size = { width: 200, height: 100 }
    expect(alignedCorner(size, canvas, 'bottom', 'end', { x: 0, y: 0 }, true)).toEqual({ left: 1720, top: 980 })
    expect(alignedCorner(size, canvas, 'left', 'start', { x: 10, y: 0 }, true)).toEqual({ left: 10, top: 0 })
    expect(alignedCorner(size, canvas, 'center', 'start', { x: 0, y: 0 }, true)).toEqual({ left: 860, top: 490 })
    // Against an item: outside its edges.
    const item = { left: 500, top: 500, width: 100, height: 100 }
    expect(alignedCorner(size, item, 'top', 'start', { x: 0, y: 0 }, false)).toEqual({ left: 500, top: 400 })
    expect(alignedCorner(size, item, 'right', 'middle', { x: 0, y: 0 }, false)).toEqual({ left: 600, top: 500 })
  })

  it("turns an align point into a box and back, whatever the item's align is", () => {
    const size = { width: 300, height: 100 }
    expect(boxOf({ x: 250, y: 250, align: 'center' }, size)).toEqual({ left: 100, top: 200, width: 300, height: 100 })
    expect(pointOf(100, 200, 'center', size)).toEqual({ x: 250, y: 250 })
    expect(pointOf(100, 200, 'bottom_right', size)).toEqual({ x: 400, y: 300 })
  })
})
