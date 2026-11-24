/**
 * Follows the drag `e` starts on its element: `move` for each pointer move, then `end` once when the browser takes
 * the pointer back, whether it was released, cancelled, or lost to another window (Alt+Tab, a system dialog).
 */
export function followDrag(e: PointerEvent, move: (ev: PointerEvent) => void, end: () => void) {
  const target = e.currentTarget as HTMLElement
  target.setPointerCapture(e.pointerId)
  target.addEventListener('pointermove', move)
  target.addEventListener(
    'lostpointercapture',
    () => {
      target.removeEventListener('pointermove', move)
      end()
    },
    { once: true },
  )
}
