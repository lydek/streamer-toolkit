// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'
import { i18n } from '../i18n'
import RedemptionList from './RedemptionList.vue'
import type { RedemptionRecord } from './redemptions'

const record = (over: Partial<RedemptionRecord> = {}): RedemptionRecord => ({
  id: 'r1',
  user_login: 'viewer',
  user_name: 'Viewer',
  reward_title: 'Hydrate',
  reward_cost: 100,
  user_input: '',
  status: 'unfulfilled',
  redeemed_at: '2026-11-16T10:00:00.000Z',
  updated_at: '2026-11-16T10:00:00.000Z',
  played: [],
  ...over,
})

const list = (records: RedemptionRecord[]) =>
  mount(RedemptionList, { props: { records, empty: '沒有紀錄' }, global: { plugins: [i18n] } })

it('shows the empty text when there are no records', () => {
  expect(list([]).text()).toBe('沒有紀錄')
})

it('shows viewer and Twitch text as text, never as markup', () => {
  const html = '<img src=x onerror="alert(1)"><b>bold</b>'
  const wrapper = list([record({ user_name: html, reward_title: html, user_input: html })])
  expect(wrapper.find('img').exists()).toBe(false)
  expect(wrapper.find('b').exists()).toBe(false)
  expect(wrapper.find('.input').text()).toBe(html)
  expect(wrapper.find('.name').text()).toBe(html)
})

it('names the status in words with its icon, and leaves out an empty input', () => {
  const wrapper = list([record({ status: 'canceled' }), record({ id: 'r2', status: 'unknown', user_input: 'hi' })])
  const statuses = wrapper.findAll('.status')
  expect(statuses.map((s) => s.text())).toEqual(['已退還', '不明'])
  expect(statuses[0].find('i').attributes('aria-hidden')).toBe('true')
  expect(wrapper.findAll('.input').map((p) => p.text())).toEqual(['hi'])
  expect(wrapper.find('time').attributes('datetime')).toBe('2026-11-16T10:00:00.000Z')
})

it('says what each redemption played: each item and its result, — for nothing, 沒有紀錄 before this was kept', () => {
  const rows = list([
    record({ id: 'a', played: [{ what: 'card', result: 'played' }, { what: 'media', result: 'file_missing' }] }),
    record({ id: 'b', played: [] }),
    record({ id: 'c', played: null }),
  ]).findAll('.played')
  expect(rows.map((r) => r.text())).toEqual(['播放：集點卡已播、影片或 GIF：檔案不見了', '播放：—', '播放：沒有紀錄'])
})
