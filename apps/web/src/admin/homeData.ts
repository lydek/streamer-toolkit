import { onBeforeUnmount, shallowRef, watch } from 'vue'
import { type Schemas, api, client } from './api'
import type { useDrawer } from './drawer'
import type { FeedItem } from './feed'
import { RECENT, type RedemptionRecord, noFilters, redemptionQuery } from './redemptions'

/**
 * What the home page reads for its tiles and drawers: the latest redemptions, today's check-ins, the sound
 * library, the music playlists and this stream's feed. The page reloads them from its live messages; a drawer
 * opening reloads its own. `onError` reports a failed playlist read in the open drawer.
 */
export function useHomeData(drawer: ReturnType<typeof useDrawer>, onError: (e: unknown) => void) {
  // The latest records, for the redemption tile and drawer; reloaded on every change and reconnect.
  const recent = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; items: RedemptionRecord[] }>({
    state: 'loading',
  })
  let recentRequest = 0
  async function loadRecent() {
    const mine = ++recentRequest
    try {
      const page = await api(client.GET('/api/admin/redemptions', { params: { query: redemptionQuery(noFilters(), 0, RECENT) } }))
      if (mine === recentRequest) recent.value = { state: 'ready', items: page.items }
    } catch {
      if (mine === recentRequest) recent.value = { state: 'failed' }
    }
  }
  // Today's check-ins, for the check-in tile and drawer; reloaded on every check-in and reconnect.
  const checkinToday = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; today: Schemas['CheckinToday'] }>({
    state: 'loading',
  })
  let checkinRequest = 0
  async function loadCheckinToday() {
    const mine = ++checkinRequest
    try {
      const today = await api(client.GET('/api/admin/checkin/today'))
      if (mine === checkinRequest) checkinToday.value = { state: 'ready', today }
    } catch {
      if (mine === checkinRequest) checkinToday.value = { state: 'failed' }
    }
  }
  // The sound library, for the sound tile and drawer; reloaded when the drawer opens.
  const sounds = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; items: Schemas['Sound'][] }>({
    state: 'loading',
  })
  async function loadSounds() {
    try {
      sounds.value = { state: 'ready', items: (await api(client.GET('/api/admin/sounds'))).sounds }
    } catch {
      sounds.value = { state: 'failed' }
    }
  }
  watch(drawer.current, (id) => id === 'sound' && void loadSounds())
  // The playlists for the music drawer; reloaded when it opens.
  const playlists = shallowRef<Schemas['MusicPlaylist'][]>([])
  async function loadPlaylists() {
    try {
      playlists.value = (await api(client.GET('/api/admin/music'))).playlists
    } catch (e) {
      onError(e)
    }
  }
  watch(drawer.current, (id) => id === 'bgm' && void loadPlaylists())
  // This stream's alerts, redemptions and check-ins with what became of them; reloaded (at most once a second) when
  // a live message may have changed them.
  const feed = shallowRef<{ state: 'loading' | 'failed' } | { state: 'ready'; items: FeedItem[] }>({ state: 'loading' })
  let feedTimer: ReturnType<typeof setTimeout> | undefined
  async function loadFeed() {
    feedTimer = undefined
    try {
      feed.value = { state: 'ready', items: await api(client.GET('/api/admin/feed')) }
    } catch {
      feed.value = { state: 'failed' }
    }
  }
  const reloadFeedSoon = () => (feedTimer ??= setTimeout(() => void loadFeed(), 1000))
  onBeforeUnmount(() => clearTimeout(feedTimer))
  return { recent, loadRecent, checkinToday, loadCheckinToday, sounds, loadSounds, playlists, feed, reloadFeedSoon }
}
