/*
 * OBS keeps text colors as integers with the bytes in reverse: 0x00BBGGRR for the Windows text source (`color`),
 * 0xAABBGGRR for the FreeType one (`color1`, opaque here). The form uses `#rrggbb`.
 */

const hex2 = (n: number) => n.toString(16).padStart(2, '0')

/** `0x..BBGGRR` → `#rrggbb` (the alpha byte is ignored). */
export function hexOf(color: number): string {
  return `#${hex2(color & 0xff)}${hex2((color >> 8) & 0xff)}${hex2((color >> 16) & 0xff)}`
}

/** `#rrggbb` → `0xBBGGRR`, plus an opaque alpha byte for `color1`. */
export function colorOf(hex: string, withAlpha: boolean): number {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return ((withAlpha ? 0xff000000 : 0) | (b << 16) | (g << 8) | r) >>> 0
}
