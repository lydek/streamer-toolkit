import { computed, shallowRef } from 'vue'
import { type Schemas, api, client } from './api'

/*
 * Every feature of the design, for the feature page in settings (docs/scope.md 階段 0). A feature owns one or more
 * pages (tabs.ts paths); a built feature is a Host module (`module`) and can be turned on and off; the rest show
 * 「還沒有這個功能」. Dependencies live here, per feature; there is no rules engine.
 */

export interface Feature {
  id: string
  group: string
  icon: string
  pages: string[]
  /** The Host module behind a built feature; absent while it is not built. */
  module?: string
  /** Always on (home). */
  fixed?: boolean
  needs?: 'obs' | 'program' | 'speech'
  /** Twitch scopes it adds, asked for the first time it is turned on. */
  scopes?: string[]
  /** Features it cannot work without. */
  requires?: string[]
  /** Features that keep working but lose something when this one is off (i18n key `features.<id>.affects`). */
  affects?: string[]
}

export const FEATURES: Feature[] = [
  { id: 'home', group: 'home', icon: 'pi pi-home', pages: ['/'], fixed: true },
  { id: 'streamInfo', group: 'home', icon: 'pi pi-pencil', pages: ['/stream-info'], scopes: ['channel:manage:broadcast'] },
  { id: 'rehearsal', group: 'home', icon: 'pi pi-play-circle', pages: ['/rehearsal'], fixed: true },
  { id: 'songs', group: 'song', icon: 'pi pi-microphone', pages: ['/songs', '/song-list'] },
  { id: 'singingBooth', group: 'song', icon: 'pi pi-headphones', pages: ['/singing-booth'], needs: 'program', requires: ['songs'] },
  { id: 'chat', group: 'chat', icon: 'pi pi-comments', pages: ['/chat-log', '/chat', '/chat-window'], module: 'chat' },
  { id: 'chatFilter', group: 'chat', icon: 'pi pi-filter', pages: ['/chat-filter'], scopes: ['moderator:manage:chat_messages'] },
  { id: 'bot', group: 'chat', icon: 'pi pi-reply', pages: ['/commands', '/timers', '/greetings'], scopes: ['user:write:chat'] },
  { id: 'alerts', group: 'interact', icon: 'pi pi-bell', pages: ['/alerts'], module: 'alerts' },
  { id: 'polls', group: 'interact', icon: 'pi pi-chart-bar', pages: ['/polls'], scopes: ['channel:manage:polls', 'channel:manage:predictions'] },
  { id: 'checkin', group: 'interact', icon: 'pi pi-check-square', pages: ['/checkin', '/card-style'], module: 'checkin' },
  { id: 'rewards', group: 'interact', icon: 'pi pi-star', pages: ['/rewards'], scopes: ['channel:manage:redemptions'], affects: ['songs'] },
  { id: 'redemption', group: 'interact', icon: 'pi pi-gift', pages: ['/redemptions'], module: 'redemption' },
  { id: 'counters', group: 'interact', icon: 'pi pi-hashtag', pages: ['/counters'], module: 'counter' },
  { id: 'scenes', group: 'media', icon: 'pi pi-images', pages: ['/scenes'], module: 'scene', needs: 'obs', affects: ['music'] },
  { id: 'music', group: 'media', icon: 'pi pi-headphones', pages: ['/music'], module: 'bgm' },
  { id: 'sound', group: 'media', icon: 'pi pi-volume-up', pages: ['/sounds'], module: 'sound' },
  { id: 'tts', group: 'media', icon: 'pi pi-comment', pages: ['/tts'], needs: 'speech' },
  { id: 'screenTools', group: 'media', icon: 'pi pi-desktop', pages: ['/screen-tools'], module: 'screen-tools' },
  { id: 'avatar', group: 'media', icon: 'pi pi-face-smile', pages: ['/avatar'], needs: 'program' },
  { id: 'shoutouts', group: 'media', icon: 'pi pi-video', pages: ['/shoutouts'], scopes: ['moderator:manage:shoutouts'] },
  { id: 'imageSizes', group: 'media', icon: 'pi pi-image', pages: ['/image-sizes'] },
  { id: 'viewers', group: 'viewers', icon: 'pi pi-users', pages: ['/viewers'], scopes: ['moderation:read', 'channel:read:vips'] },
  { id: 'roles', group: 'viewers', icon: 'pi pi-tags', pages: ['/roles'], requires: ['viewers'] },
  { id: 'leaderboard', group: 'viewers', icon: 'pi pi-trophy', pages: ['/leaderboard'], fixed: true },
  { id: 'credits', group: 'viewers', icon: 'pi pi-list', pages: ['/credits'] },
  { id: 'raids', group: 'viewers', icon: 'pi pi-send', pages: ['/raids'], scopes: ['channel:manage:raids'] },
]

/** On: always-on, or a built feature whose module is enabled. A feature not built yet is never on. */
export function isOn(f: Feature, modules: Schemas['ModuleState'][]): boolean {
  return !!f.fixed || (!!f.module && !!modules.find((m) => m.id === f.module)?.enabled)
}

/** What turning `f` on needs first: features it requires that are off, and scopes not granted yet. */
export function turnOnPlan(f: Feature, on: (f: Feature) => boolean, granted: Set<string>, all = FEATURES) {
  return {
    requires: (f.requires ?? []).map((id) => all.find((x) => x.id === id)!).filter((d) => !on(d)),
    scopes: (f.scopes ?? []).filter((s) => !granted.has(s)),
  }
}

/** What turning `f` off takes with it: features that require it (they go off too) and ones it affects. */
export function turnOffPlan(f: Feature, on: (f: Feature) => boolean, all = FEATURES) {
  return {
    dependents: all.filter((x) => x.requires?.includes(f.id) && on(x)),
    affected: (f.affects ?? []).map((id) => all.find((x) => x.id === id)!).filter(on),
  }
}

/**
 * Pages of built features whose module is listed as disabled: the tabs and sub-tabs hide them. A module not
 * listed (not loaded yet, or the load failed) hides nothing.
 */
export function hiddenPages(modules: Schemas['ModuleState'][], all = FEATURES): Set<string> {
  const off = new Set(modules.filter((m) => !m.enabled).map((m) => m.id))
  return new Set(all.filter((f) => f.module && off.has(f.module)).flatMap((f) => f.pages))
}

// The module states shared by the app bar and settings; settings hands over each list it loads.
const modules = shallowRef<Schemas['ModuleState'][]>()
let loading: Promise<void> | undefined

/** Settings passes the module states it just loaded, so the tabs follow a switch at once. */
export function shareModules(list: Schemas['ModuleState'][]) {
  modules.value = list
}

async function reloadModules() {
  modules.value = await api(client.GET('/api/admin/modules'))
}

/** The shared module states, loaded on first use (a failed load leaves every page shown). */
export function useModules() {
  loading ??= reloadModules().catch(() => {
    loading = undefined
  })
  return { modules, hidden: computed(() => hiddenPages(modules.value ?? [])) }
}
