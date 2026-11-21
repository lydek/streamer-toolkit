import { i18n } from '../i18n'
import type { Schemas } from './api'

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
  const what = r.triggered === 'sound' ? t('feed.triggered.sound', { sound: r.sound ?? '' }) : t(`feed.triggered.${r.triggered}`)
  return t('feed.result', { what, outcome: t(`feed.outcome.${r.outcome}`) })
}

/** The home tile's line: how many, and the latest. */
export function feedLine(items: FeedItem[]): string {
  const { t } = i18n.global
  return items.length ? t('feed.line', { n: items.length, latest: feedText(items[0]) }) : t('feed.empty')
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
