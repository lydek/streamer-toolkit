// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { i18n } from '../i18n'
import ChatLine from './ChatLine.vue'
import { DEFAULT_DISPLAY, avatars, badgeImages, chatDisplay, sharedChannels } from './chatDisplay'
import type { LoggedChat } from './chatLog'

const msg = (over: Partial<LoggedChat['payload']> = {}, deleted = false): LoggedChat => ({
  id: 'c1',
  type: 'chat.message',
  version: 1,
  seq: 1,
  occurredAt: '2026-11-19T10:00:00.000Z',
  source: 'twitch',
  payload: { userId: '7', userLogin: 'fan', userName: 'Fan', color: null, text: '<b>hi</b>', fragments: [], badges: [], highlighted: false, ...over },
  deleted,
})
const line = (m: LoggedChat, broadcasterId?: string) => mount(ChatLine, { props: { message: m, broadcasterId }, global: { plugins: [i18n] } })
const t = i18n.global.t

afterEach(() => {
  chatDisplay.value = DEFAULT_DISPLAY
  badgeImages.value = new Map()
  avatars.value = new Map()
})

describe('ChatLine', () => {
  it('shows the avatar and the badges Twitch gave images for, and viewer text as text', () => {
    avatars.value = new Map([['7', 'https://static-cdn.jtvnw.net/a.png']])
    badgeImages.value = new Map([['moderator/1', { setId: 'moderator', id: '1', title: 'Moderator', url: 'https://static-cdn.jtvnw.net/m' }]])
    const w = line(
      msg({
        twitchBadges: [
          { setId: 'moderator', id: '1', info: '' },
          { setId: 'unknown', id: '1', info: '' },
        ],
        bits: 100,
        reply: { messageId: 'p', userId: '8', userLogin: 'kai', userName: 'Kai' },
      }),
    )
    const images = w.findAll('img').map((i) => i.attributes('src'))
    expect(images).toEqual(['https://static-cdn.jtvnw.net/a.png', 'https://static-cdn.jtvnw.net/m'])
    expect(w.text()).toContain('<b>hi</b>')
    expect(w.find('b').exists()).toBe(false)
    expect(w.text()).toContain(t('chat.replyTo', { name: 'Kai' }))
    expect(w.text()).toContain(t('chat.bits', { bits: 100 }))
  })

  it('follows the display settings and marks deleted, shared and own lines', () => {
    avatars.value = new Map([['7', 'https://static-cdn.jtvnw.net/a.png']])
    chatDisplay.value = { ...DEFAULT_DISPLAY, avatars: false, time: false, style: 'compact' }
    const shared = line(msg({ sharedFrom: { userId: '99', userLogin: 'pianocat', userName: 'PianoCat' } }))
    expect(shared.find('img').exists()).toBe(false)
    expect(shared.find('time').exists()).toBe(false)
    expect(shared.find('.line').classes()).toContain('style-compact')
    expect(shared.text()).toContain(t('chat.sharedFrom', { channel: 'PianoCat' }))
    expect(line(msg({}, true)).text()).toContain(t('chat.deleted'))
    expect(line(msg(), '7').find('.badge').text()).toBe(t('chat.broadcaster'))
  })

  it('lists the other channels of a shared chat, latest first', () => {
    const from = (name: string) => msg({ sharedFrom: { userId: name, userLogin: name, userName: name } })
    expect(sharedChannels([from('A'), msg(), from('B'), from('A')])).toEqual(['A', 'B'])
    expect(sharedChannels([msg()])).toEqual([])
  })
})
