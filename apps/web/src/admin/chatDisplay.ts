import { shallowRef } from 'vue'
import { type Schemas, api, client } from './api'
import type { LoggedChat } from './chatLog'

export type ChatDisplay = Schemas['ChatDisplay']

export const DEFAULT_DISPLAY: ChatDisplay = { style: 'standard', avatars: true, badges: true, time: true, textSize: 100 }
export const TEXT_SIZES = [80, 90, 100, 110, 120, 130, 140, 150]

/** How the admin UI shows chat; shared by the chat log page and the docked panel, loaded once. */
export const chatDisplay = shallowRef<ChatDisplay>(DEFAULT_DISPLAY)
let displayLoaded: Promise<void> | undefined
export function loadChatDisplay(): Promise<void> {
  displayLoaded ??= api(client.GET('/api/admin/chat-display'))
    .then((initial) => {
      if (initial && !revision) {
        saved = initial
        chatDisplay.value = initial
      }
    })
    .catch((error) => {
      displayLoaded = undefined
      throw error
    })
  return displayLoaded
}

let saved = chatDisplay.value
let write = Promise.resolve()
let revision = 0

export async function saveChatDisplay(change: Partial<ChatDisplay>): Promise<void> {
  await loadChatDisplay()
  const next = { ...chatDisplay.value, ...change }
  const version = ++revision
  chatDisplay.value = next
  const request = write.then(async () => {
    await api(client.PUT('/api/admin/chat-display', { body: next }))
    saved = next
  })
  write = request.catch(() => {})
  try {
    await request
  } catch (error) {
    if (revision === version) chatDisplay.value = saved
    throw error
  }
}

export const badgeImages = shallowRef(new Map<string, Schemas['BadgeImage']>())
let owner: string | undefined
let badgeRequest = 0
export function loadBadges(id?: string) {
  if (id === owner) return
  owner = id
  const version = ++badgeRequest
  badgeImages.value = new Map()
  if (!id) return
  async function fetchBadges() {
    if (version !== badgeRequest) return
    const list = await api(client.GET('/api/admin/chat/badges')).catch(() => [])
    if (version !== badgeRequest) return
    if (list.length) {
      badgeImages.value = new Map(list.map((b) => [`${b.setId}/${b.id}`, b]))
    } else {
      setTimeout(fetchBadges, RETRY_MS)
    }
  }
  void fetchBadges()
}

/**
 * Avatars by Twitch user ID, asked for in batches as messages show up. The Host looks a new viewer's avatar up in
 * the background, so one not found yet is asked again a little later, a few times.
 */
export const avatars = shallowRef(new Map<string, string>())
const asked = new Set<string>()
const tries = new Map<string, number>()
const RETRY_MS = 30_000
const TRIES = 3
let wanted = new Set<string>()
let timer: ReturnType<typeof setTimeout> | undefined
const BATCH = 200

export function wantAvatars(messages: LoggedChat[]) {
  for (const m of messages) {
    const id = m.payload.userId
    if (/^\d+$/.test(id) && !asked.has(id)) wanted.add(id)
  }
  scheduleAvatars()
}

function scheduleAvatars() {
  if (!wanted.size || timer) return
  timer = setTimeout(() => {
    timer = undefined
    const ids = [...wanted].slice(0, BATCH)
    wanted = new Set([...wanted].slice(BATCH))
    for (const id of ids) asked.add(id)
    // A failed request counts as one try for every ID in it, so a page left open still retries.
    void api(client.GET('/api/admin/viewers/avatars', { params: { query: { ids: ids.join(',') } } }))
      .catch(() => ({}) as Record<string, string>)
      .then((found) => {
        if (Object.keys(found).length) avatars.value = new Map([...avatars.value, ...Object.entries(found)])
        for (const id of ids.filter((id) => !(id in found))) {
          const n = (tries.get(id) ?? 0) + 1
          tries.set(id, n)
          if (n < TRIES) setTimeout(() => { wanted.add(id); scheduleAvatars() }, RETRY_MS)
        }
      })
    scheduleAvatars()
  }, 300)
}

/** The other channels in a shared chat, from the messages that came from them; latest first. */
export function sharedChannels(messages: LoggedChat[]): string[] {
  const names: string[] = []
  for (let i = messages.length - 1; i >= 0; i--) {
    const from = messages[i].payload.sharedFrom
    if (from && !names.includes(from.userName)) names.push(from.userName)
  }
  return names
}
