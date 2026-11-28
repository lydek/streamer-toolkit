import { alertTitle } from '../overlay/alerts/alerts'
import type { Schemas } from './api'
import { counterTile } from './counters'
import { type FeedItem, feedTile } from './feed'
import { type Line, type LiveState, TILES } from './home'
import { FEATURES } from './features'
import { GROUPS } from './tabs'
import { playerLine } from './music'
import { subjectText } from './queue'
import type { RedemptionRecord } from './redemptions'

/*
 * The home tiles' one-line summaries that depend only on what they are given (the page passes `t`). The ones that
 * read the page's own state (本場直播, 音訊混音) stay in HomeView.
 */
type T = (key: string, values?: Record<string, unknown>) => string
type Loaded<V> = { state: 'loading' | 'failed' } | ({ state: 'ready' } & V)

export const count = (n: number) => n.toLocaleString('zh-TW')

export function checkinLine(c: Loaded<{ today: Schemas['CheckinToday'] }>, t: T): Line {
  if (c.state !== 'ready') return { state: c.state }
  const last = c.today.recent[0]
  return {
    state: 'ready',
    num: count(c.today.count),
    unit: t('home.units.checkin'),
    text: last ? t('home.latest', { text: last.user_name }) : t('checkin.page.empty'),
  }
}

export function sceneLine(obs: { scene?: string | null; scenes: unknown[] } | undefined, t: T): Line {
  if (!obs?.scene) return { state: 'ready', text: t('scene.notConnected') }
  return { state: 'ready', num: obs.scene, text: t('home.sceneCount', { n: obs.scenes.length }) }
}

export function bgmLine(p: Schemas['BgmPlayerPayload'], playlists: Schemas['MusicPlaylist'][], t: T): Line {
  if (!p.track || p.state === 'stopped' || !p.onAir) return { state: 'ready', text: playerLine(p) }
  const name = playlists.find((l) => l.id === p.playlist)?.name
  const order = t(p.shuffle ? 'music.rules.shuffle' : 'music.rules.inOrder')
  const text = [p.state === 'paused' ? t('music.player.pausedShort') : null, name, order].filter(Boolean).join(' · ')
  return { state: 'ready', num: p.track.title, text }
}

/** The 畫面工具 tile: whether the break card is on screen. */
export function breakLine(card: Schemas['BreakPayload'], t: T): Line {
  if (!card.shown) return { state: 'ready', num: t('screenTools.break.hiddenShort'), text: t('screenTools.break.hidden') }
  return { state: 'ready', num: t('screenTools.break.shownShort'), text: card.text || t('screenTools.break.heading') }
}

/** The small 呈現佇列 tile: how many wait, and what plays. */
export function queueLine(q: Schemas['QueueStatus'] | undefined, t: T): Line {
  if (!q) return { state: 'loading' }
  const text = q.current ? `${t('queue.playing')}：${subjectText(q.current.subject)}` : t('home.queueIdle')
  return { state: 'ready', num: count(q.waiting.length), unit: t('home.units.queue'), text }
}

export function recentLine(r: Loaded<{ items: RedemptionRecord[] }>, t: T): Line {
  if (r.state !== 'ready') return { state: r.state }
  const last = r.items[0]
  if (!last) return { state: 'ready', text: t('redemption.empty') }
  return {
    state: 'ready',
    num: count(r.items.length),
    unit: t('home.units.redemption'),
    text: `${last.user_name}：${last.reward_title}`,
  }
}

/** What the tiles' lines are drawn from; 本場直播 and 音訊混音 come ready-made from the page. */
export interface TileData {
  live: LiveState
  recent: Loaded<{ items: RedemptionRecord[] }>
  checkinToday: Loaded<{ today: Schemas['CheckinToday'] }>
  sounds: Loaded<{ items: Schemas['Sound'][] }>
  playlists: Schemas['MusicPlaylist'][]
  feed: Loaded<{ items: FeedItem[] }>
  stats: Line
  mixer: Line
}

/** Every home tile's one-line summary, by tile ID. */
export function tileLines(d: TileData, t: T): Record<string, Line> {
  const { status, lastChat, lastAlert } = d.live
  // The stage blocks show their content on home itself; in edit mode they show only their name.
  const block: Line = { state: 'ready', text: '' }
  return {
    pending: block,
    checklist: block,
    summary: block,
    stats: d.stats,
    queue: queueLine(d.live.queue, t),
    chat:
      status === 'open'
        ? { state: 'ready', text: lastChat ? `${lastChat.payload.userName}：${lastChat.payload.text}` : t('home.chatEmpty') }
        : { state: status },
    alerts:
      status === 'open'
        ? { state: 'ready', text: lastAlert ? alertTitle(lastAlert.payload) : t('alerts.empty') }
        : { state: status },
    redemption: recentLine(d.recent, t),
    checkin: checkinLine(d.checkinToday, t),
    sound:
      d.sounds.state === 'ready'
        ? d.sounds.items.length
          ? {
              state: 'ready',
              num: count(d.sounds.items.length),
              unit: t('home.units.sound'),
              text: d.sounds.items.map((s) => s.label).join('、'),
            }
          : { state: 'ready', text: t('sound.empty') }
        : { state: d.sounds.state },
    scene: status === 'open' ? sceneLine(d.live.obsState, t) : { state: status },
    mixer: status === 'open' ? d.mixer : { state: status },
    bgm: status !== 'open' ? { state: status } : d.live.bgm ? bgmLine(d.live.bgm.payload, d.playlists, t) : { state: 'loading' },
    feed: d.feed.state === 'ready' ? feedTile(d.feed.items) : { state: d.feed.state },
    counter:
      status !== 'open'
        ? { state: status }
        : d.live.counters
          ? counterTile(d.live.counters)
          : { state: 'loading' },
    rehearsal: { state: 'ready', text: t('rehearsal.tile') },
    'image-sizes': { state: 'ready', text: t('imageSizes.tile') },
    'stream-info': { state: 'ready', text: t('streamInfo.tile') },
    rewards: { state: 'ready', text: t('rewards.tile') },
    polls: { state: 'ready', text: t('polls.tile') },
    commands: { state: 'ready', text: t('bot.tile') },
    songs: { state: 'ready', text: t('songs.tile') },
    'screen-tools':
      status !== 'open'
        ? { state: status }
        : d.live.breakCard
          ? breakLine(d.live.breakCard, t)
          : { state: 'loading' },
  }
}

/** A tile's (and its drawer's) name: its feature's, its module's, its block's, or its page's. */
export function tileName(id: string, t: T, te: (key: string) => boolean): string {
  const unbuilt = TILES.find((tile) => tile.id === id)?.unbuilt
  if (unbuilt) return t(unbuilt.label)
  // A module's tile goes by its feature's name, as its page does (場景與模式, 休息卡片); the ID stays the module's.
  const module = TILES.find((tile) => tile.id === id)?.module ?? id
  const feature = FEATURES.find((f) => f.module === module)
  if (feature) return t(`features.${feature.id}.name`)
  if (te(`modules.names.${id}`)) return t(`modules.names.${id}`)
  if (te(`home.blocks.${id}`)) return t(`home.blocks.${id}`)
  // A tile that is no module's (e.g. 演練) is named after its page.
  const page = GROUPS.flatMap((g) => g.pages).find((p) => p.path === `/${id}`)
  return page ? t(page.label) : id
}
