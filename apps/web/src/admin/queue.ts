import { i18n } from '../i18n'
import type { Schemas } from './api'

/** About how long until everything in the queue has played, in ms, at `now` (the Host runs on this machine). */
export function waitMs(status: Schemas['QueueStatus'] | undefined, now: number): number {
  if (!status) return 0
  const gap = status.gapMs
  const current = status.current
    ? Math.max(0, Date.parse(status.current.startedAt) + status.current.showMs + gap - now)
    : 0
  return status.waiting.reduce((sum, w) => sum + w.showMs + gap, current)
}

/** 特效：阿凱 訂閱了, 打卡卡片：小魚 7/10, 音效：喝水（橘子兌換）, 影片：貓咪（橘子兌換）; names are raw text, escaped by the template. */
export function subjectText(s: Schemas['Subject']): string {
  const { t } = i18n.global
  switch (s.kind) {
    case 'alert':
      return t('queue.item.alert', {
        viewer: s.viewer || t('queue.anonymous'),
        action: t(`queue.action.${s.alert}`),
      })
    case 'card':
      return t('queue.item.card', { viewer: s.viewer, stamps: s.stamps, slots: s.slots })
    case 'sound':
      return t('queue.item.sound', { sound: s.sound, viewer: s.viewer })
    case 'media':
      return t('queue.item.media', { media: s.media, viewer: s.viewer })
  }
}
