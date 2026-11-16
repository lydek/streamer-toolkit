/** Plays `url` at `volume` (0–100). OBS lets browser sources play without a click; elsewhere a refusal only means silence. */
export function playSound(url: string, volume: number): HTMLAudioElement {
  const audio = new Audio(url)
  audio.volume = Math.min(Math.max(volume, 0), 100) / 100
  audio.play()?.catch(() => {})
  return audio
}
