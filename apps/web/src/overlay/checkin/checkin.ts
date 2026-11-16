/** The URL when it is on Twitch's image CDN (the Host checks too); anything else shows no image. */
export function twitchImage(url: string): string | undefined {
  try {
    const u = new URL(url)
    return u.protocol === 'https:' && u.host === 'static-cdn.jtvnw.net' ? u.href : undefined
  } catch {
    return undefined
  }
}
