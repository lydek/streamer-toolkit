import type { Schemas } from './api'

/*
 * One-time relative alignment for scene items: put an item against the canvas (inside its edges) or against another
 * item (outside its edges; `center` overlaps its middle), along one side, then nudge it by an offset. Computed once:
 * nothing follows the reference afterwards. Positions in OBS are the item's `align` point, so boxes are worked out
 * from that point and the item's size first, and the answer goes back to the item's own point.
 */

export type Side = 'top' | 'bottom' | 'left' | 'right' | 'center'
export type Cross = 'start' | 'middle' | 'end'
export interface Box {
  left: number
  top: number
  width: number
  height: number
}

// How far the align point is from the box's left and top, as a share of its width and height.
const SHARE: Record<Schemas['Align'], [number, number]> = {
  top_left: [0, 0],
  top: [0.5, 0],
  top_right: [1, 0],
  left: [0, 0.5],
  center: [0.5, 0.5],
  right: [1, 0.5],
  bottom_left: [0, 1],
  bottom: [0.5, 1],
  bottom_right: [1, 1],
}

/** The item's box on the canvas from where its align point sits and its size. */
export function boxOf(p: Schemas['Placement'], size: Schemas['ItemSize']): Box {
  const [sx, sy] = SHARE[p.align]
  return { left: p.x - sx * size.width, top: p.y - sy * size.height, width: size.width, height: size.height }
}

/** Where the item's align point goes for its box to start at `left`, `top`. */
export function pointOf(left: number, top: number, align: Schemas['Align'], size: Schemas['ItemSize']) {
  const [sx, sy] = SHARE[align]
  return { x: Math.round(left + sx * size.width), y: Math.round(top + sy * size.height) }
}

/**
 * The top-left corner for a box of `size` put against `ref`: inside its edges for the canvas (`inside`), outside
 * them for another item, `center` on its middle; `cross` places it along that side; then the offsets are added.
 */
export function alignedCorner(size: Schemas['ItemSize'], ref: Box, side: Side, cross: Cross, offset: { x: number; y: number }, inside: boolean) {
  const along = (start: number, length: number, own: number) =>
    cross === 'start' ? start : cross === 'end' ? start + length - own : start + (length - own) / 2
  let left: number
  let top: number
  if (side === 'center') {
    left = ref.left + (ref.width - size.width) / 2
    top = ref.top + (ref.height - size.height) / 2
  } else if (side === 'top' || side === 'bottom') {
    left = along(ref.left, ref.width, size.width)
    const outsideTop = ref.top - size.height
    const outsideBottom = ref.top + ref.height
    top = side === 'top' ? (inside ? ref.top : outsideTop) : inside ? outsideBottom - size.height : outsideBottom
  } else {
    top = along(ref.top, ref.height, size.height)
    const outsideLeft = ref.left - size.width
    const outsideRight = ref.left + ref.width
    left = side === 'left' ? (inside ? ref.left : outsideLeft) : inside ? outsideRight - size.width : outsideRight
  }
  return { left: left + offset.x, top: top + offset.y }
}
