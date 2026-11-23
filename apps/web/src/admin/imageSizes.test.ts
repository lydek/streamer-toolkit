import { describe, expect, it } from 'vitest'
import { centeredCrop, fitSize, moveCrop, resizeCrop, setCropField } from './imageSizes'

describe('crop', () => {
  it('starts as the largest square in the middle', () => {
    expect(centeredCrop(300, 120)).toEqual({ x: 90, y: 0, w: 120, h: 120 })
    expect(centeredCrop(100, 251)).toEqual({ x: 0, y: 75, w: 100, h: 100 })
  })

  it('moves inside the picture', () => {
    const c = { x: 90, y: 0, w: 120, h: 60 }
    expect(moveCrop(c, -500, 90, 300, 120)).toEqual({ x: 0, y: 60, w: 120, h: 60 })
    expect(moveCrop(c, 500, 0, 300, 120)).toEqual({ x: 180, y: 0, w: 120, h: 60 })
  })

  it('resizes from the dragged corner, the opposite corner kept, inside the picture and at least 8 px', () => {
    const c = { x: 100, y: 20, w: 50, h: 40 }
    expect(resizeCrop(c, 'se', 500, 500, 300, 120)).toEqual({ x: 100, y: 20, w: 200, h: 100 })
    expect(resizeCrop(c, 'se', -500, -500, 300, 120)).toEqual({ x: 100, y: 20, w: 8, h: 8 })
    expect(resizeCrop(c, 'nw', 30, -10, 300, 120)).toEqual({ x: 130, y: 10, w: 20, h: 50 })
    expect(resizeCrop(c, 'nw', -500, -500, 300, 120)).toEqual({ x: 0, y: 0, w: 150, h: 60 })
  })

  it('keeps a locked ratio while resizing, through the 8 px minimum and the picture edge', () => {
    const c = { x: 0, y: 0, w: 200, h: 100 }
    expect(resizeCrop(c, 'se', -100, 0, 300, 300, 2)).toEqual({ x: 0, y: 0, w: 100, h: 50 })
    expect(resizeCrop(c, 'se', 0, 300, 300, 300, 2)).toEqual({ x: 0, y: 0, w: 300, h: 150 })
    expect(fitSize(4, 8, 300, 300, 0.5)).toEqual({ w: 8, h: 16 })
    expect(setCropField({ x: 0, y: 0, w: 200, h: 400 }, 'h', 8, 1000, 1000, 0.5)).toEqual({ x: 0, y: 0, w: 8, h: 16 })
    expect(setCropField({ x: 0, y: 0, w: 200, h: 400 }, 'h', 300, 1000, 1000, 0.5)).toEqual({ x: 0, y: 0, w: 150, h: 300 })
  })

  it('takes typed fields: X and Y kept inside, W and H from the top-left corner', () => {
    const c = { x: 10, y: 10, w: 100, h: 50 }
    expect(setCropField(c, 'x', 999, 300, 120)).toEqual({ ...c, x: 200 })
    expect(setCropField(c, 'w', 999, 300, 120)).toEqual({ ...c, w: 290 })
    expect(setCropField(c, 'h', 2, 300, 120)).toEqual({ ...c, h: 8 })
  })

  it('stays within a picture smaller than 8 px', () => {
    expect(fitSize(1, 100, 5, 300)).toEqual({ w: 5, h: 100 })
    expect(fitSize(20, 1000, 20, 1000)).toEqual({ w: 20, h: 1000 })
  })
})
