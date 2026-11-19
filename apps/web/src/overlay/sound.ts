/** Sounds this page started that have not ended yet, so an emergency stop can silence them. */
const playing = new Set<HTMLAudioElement>()

/** Keeps track of `audio` until it ends or fails. */
export function trackSound(audio: HTMLAudioElement) {
  playing.add(audio)
  const forget = () => playing.delete(audio)
  audio.addEventListener('ended', forget)
  audio.addEventListener('error', forget)
}

/** Plays `url` at `volume` (0–100). OBS lets browser sources play without a click; elsewhere a refusal only means silence. */
export function playSound(url: string, volume: number): HTMLAudioElement {
  const audio = new Audio(url)
  audio.volume = Math.min(Math.max(volume, 0), 100) / 100
  trackSound(audio)
  audio.play()?.catch(() => {})
  return audio
}

/** Emergency stop: silences every sound this page started; each ends as if it had finished. */
export function stopSounds() {
  for (const audio of [...playing]) {
    audio.pause()
    audio.dispatchEvent(new Event('ended'))
  }
}
