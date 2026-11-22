import { describe, expect, it } from 'vitest'
import { centeredCrop, moveCrop, resizeCrop } from './imageSizes'

describe('the picked square', () => {
  it('starts centered and as large as fits', () => {
    expect(centeredCrop(300, 120)).toEqual({ x: 90, y: 0, side: 120 })
    expect(centeredCrop(100, 251)).toEqual({ x: 0, y: 75, side: 100 })
  })

  it('moves and resizes but never leaves the picture', () => {
    const c = { x: 90, y: 0, side: 120 }
    expect(moveCrop(c, -500, 50, 300, 120)).toEqual({ x: 0, y: 0, side: 120 })
    expect(moveCrop(c, 500, 0, 300, 120)).toEqual({ x: 180, y: 0, side: 120 })
    expect(resizeCrop({ x: 200, y: 10, side: 50 }, 500, 300, 120)).toEqual({ x: 200, y: 10, side: 100 })
    expect(resizeCrop({ x: 200, y: 10, side: 50 }, -500, 300, 120)).toEqual({ x: 200, y: 10, side: 8 })
  })
})
