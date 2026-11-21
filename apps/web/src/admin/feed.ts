import { i18n } from '../i18n'
import type { Schemas } from './api'
import type { Line } from './home'

export type FeedItem = Schemas['FeedItem']

/** Who did what, e.g. "阿凱 送了 3 份訂閱！". Names are viewer text: show them as text only. */
export function feedText(item: FeedItem): string {
  const { t } = i18n.global
  const name = item.viewer ?? t('alerts.anonymous')
  const e = item.event
  switch (e.kind) {
    case 'alert': {
      const n = e.amount ?? 0
      return t(`alerts.text.${e.alert}`, { name, months: n, count: n, bits: n, viewers: n })
    }
    case 'redemption':
      return t('feed.redemption', { name, reward: e.reward })
    case 'checkin':
      return t(e.trigger === 'simulation' ? 'feed.card' : 'feed.checkin', { name })
  }
}

/** What it set off and what became of it, e.g. "音效「Bell」：已播"; empty when this launch does not know. */
export function feedResult(item: FeedItem): string {
  const { t } = i18n.global
  const r = item.result
  if (!r) return ''
  const what =
    r.triggered === 'sound'
      ? t('feed.triggered.sound', { sound: r.sound ?? '' })
      : r.triggered === 'media'
        ? t('feed.triggered.media', { media: r.media ?? '' })
        : t(`feed.triggered.${r.triggered}`)
  const outcome = t(`feed.outcome.${r.outcome}`)
  return t('feed.result', { what, outcome: r.reason ? t('feed.because', { outcome, reason: t(`feed.reason.${r.reason}`) }) : outcome })
}

/** The home tile's line: how many, and the latest. */
/** The Host lists at most this many feed items. */
const FEED_MAX = 50

/** The home tile: how many items are listed ("50+" when the list is full) and the latest. */
export function feedTile(items: FeedItem[]): Line {
  const { t } = i18n.global
  if (!items.length) return { state: 'ready', text: t('feed.empty') }
  const n = items.length
  return {
    state: 'ready',
    num: n >= FEED_MAX ? `${FEED_MAX}+` : String(n),
    unit: t('home.units.feed'),
    text: t('home.latest', { text: feedText(items[0]) }),
  }
}

/** The live messages after which the feed may have changed. */
export const changesFeed = (type: string) =>
  [
    'snapshot',
    'alert.received',
    'alert',
    'redemption.changed',
    'checkin.changed',
    'checkin.stamped',
    'sound.play',
    'presentation.queue',
  ].includes(type)
