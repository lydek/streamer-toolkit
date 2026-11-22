import { describe, expect, it } from 'vitest'
import { i18n } from '../i18n'
import { type FeedItem, changesFeed, feedResult, feedText, feedTile } from './feed'

const t = i18n.global.t
const item = (p: Partial<FeedItem>): FeedItem => ({
  at: '2026-11-21T10:00:00.000Z',
  viewer: '阿凱',
  event: { kind: 'alert', alert: 'gift', amount: 3 },
  result: null,
  simulated: false,
  ...p,
})

describe('feed', () => {
  it('says who did what', () => {
    expect(feedText(item({}))).toBe(t('alerts.text.gift', { name: '阿凱', count: 3 }))
    expect(feedText(item({ viewer: null, event: { kind: 'alert', alert: 'cheer', amount: 100 } }))).toBe(
      t('alerts.text.cheer', { name: t('alerts.anonymous'), bits: 100 }),
    )
    expect(feedText(item({ viewer: '橘子', event: { kind: 'redemption', reward: '喝水' } }))).toBe(t('feed.redemption', { name: '橘子', reward: '喝水' }))
    expect(feedText(item({ viewer: '小魚', event: { kind: 'checkin', trigger: 'chat' } }))).toBe(t('feed.checkin', { name: '小魚' }))
    expect(feedText(item({ viewer: 'Kai', event: { kind: 'checkin', trigger: 'simulation' } }))).toBe(t('feed.card', { name: 'Kai' }))
  })

  it('says what it set off and what became of it, or nothing when unknown', () => {
    expect(feedResult(item({}))).toBe('')
    expect(feedResult(item({ result: { triggered: 'sound', sound: 'Bell', media: null, outcome: 'sound_missing', reason: null } }))).toBe(
      t('feed.result', { what: t('feed.triggered.sound', { sound: 'Bell' }), outcome: t('feed.outcome.sound_missing') }),
    )
    expect(feedResult(item({ result: { triggered: 'card', sound: null, media: null, outcome: 'off_air', reason: null } }))).toBe(
      t('feed.result', { what: t('feed.triggered.card'), outcome: t('feed.outcome.off_air') }),
    )
    for (const outcome of ['queued', 'played', 'declined', 'skipped', 'cleared', 'stopped'] as const) {
      expect(feedResult(item({ result: { triggered: 'alert', sound: null, media: null, outcome, reason: null } }))).toBe(
        t('feed.result', { what: t('feed.triggered.alert'), outcome: t(`feed.outcome.${outcome}`) }),
      )
    }
    expect(
      feedResult(item({ result: { triggered: 'media', sound: null, media: '貓咪', outcome: 'declined', reason: 'file_missing' } })),
    ).toBe(
      t('feed.result', {
        what: t('feed.triggered.media', { media: '貓咪' }),
        outcome: t('feed.because', { outcome: t('feed.outcome.declined'), reason: t('feed.reason.file_missing') }),
      }),
    )
  })

  it('counts and shows the latest on the tile, and knows which messages change it', () => {
    expect(feedTile([item({}), item({ viewer: '莓莓' })])).toEqual({
      state: 'ready',
      num: '2',
      unit: t('home.units.feed'),
      text: t('home.latest', { text: t('alerts.text.gift', { name: '阿凱', count: 3 }) }),
    })
    expect(feedTile(Array.from({ length: 50 }, () => item({})))).toMatchObject({ num: '50+' })
    expect(feedTile([])).toEqual({ state: 'ready', text: t('feed.empty') })
    expect(changesFeed('presentation.queue')).toBe(true)
    expect(changesFeed('chat.message')).toBe(false)
  })
})
