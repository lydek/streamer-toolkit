// @vitest-environment jsdom
// Pages drawn ahead of their backend (UI-G): they render the prototype's layout empty, with backend actions disabled.
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { Component } from 'vue'
import { i18n } from '../i18n'
import BotCommandsPage from './BotCommandsPage.vue'
import BotGreetingsPage from './BotGreetingsPage.vue'
import BotTimersPage from './BotTimersPage.vue'
import PollsPage from './PollsPage.vue'
import RewardsPage from './RewardsPage.vue'
import StreamInfoPage from './StreamInfoPage.vue'
import SongsPage from './SongsPage.vue'
import SongListPage from './SongListPage.vue'
import SingingBoothPage from './SingingBoothPage.vue'
import ShoutoutsPage from './ShoutoutsPage.vue'
import RaidsPage from './RaidsPage.vue'
import { testRouter } from './testRouter'

const t = i18n.global.t

// PrimeVue's Select reads matchMedia, which jsdom lacks.
beforeAll(() => vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} })))

async function page(component: Component, path: string) {
  const router = testRouter([{ path, component }])
  await router.push(path)
  return mount({ template: '<RouterView />' }, { global: { plugins: [router, i18n, PrimeVue] } })
}

const button = (w: Awaited<ReturnType<typeof page>>, label: string) => w.findAll('button').find((b) => b.text() === label)!

describe('直播資訊', () => {
  it('shows the form with the update disabled and no templates', async () => {
    const w = await page(StreamInfoPage, '/stream-info')
    expect(w.get('h1').text()).toBe(t('pages.streamInfo.name'))
    expect(w.find('#si-title').exists()).toBe(true)
    expect(button(w, t('streamInfo.update')).attributes('disabled')).toBeDefined()
    expect(w.get('.empty-cell').text()).toBe(t('streamInfo.templates.empty'))
  })
})

describe('頻道點數 › 獎勵', () => {
  it('shows no rewards, with adding one disabled', async () => {
    const w = await page(RewardsPage, '/rewards')
    expect(w.get('h1').text()).toBe(t('pages.rewards.name'))
    expect(w.get('.empty-cell').text()).toBe(t('rewards.empty'))
    expect(button(w, t('rewards.add')).attributes('disabled')).toBeDefined()
  })
})

describe('投票與預測', () => {
  it('shows nothing running, and a new poll form that adds up to 5 options but cannot start', async () => {
    const w = await page(PollsPage, '/polls')
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([t('polls.none.poll'), t('polls.none.prediction')])
    const add = button(w, t('polls.addOption'))
    for (let i = 0; i < 4; i++) await add.trigger('click')
    expect(w.findAll('[role="group"] input')).toHaveLength(5)
    expect(add.attributes('disabled')).toBeDefined()
    expect(button(w, t('polls.start')).attributes('disabled')).toBeDefined()
  })
})

describe('聊天機器人', () => {
  it('shows no commands or timers, with adding them disabled', async () => {
    for (const [component, path, kind] of [
      [BotCommandsPage, '/commands', 'commands'],
      [BotTimersPage, '/timers', 'timers'],
    ] as const) {
      const w = await page(component, path)
      expect(w.get('h1').text()).toBe(t(`pages.${kind}.name`))
      expect(w.get('.empty-cell').text()).toBe(t(`bot.${kind}.empty`))
      expect(button(w, t(`bot.${kind}.add`)).attributes('disabled')).toBeDefined()
    }
  })

  it('shows one message per greeting with its switch disabled, and the variables as written', async () => {
    const w = await page(BotGreetingsPage, '/greetings')
    const switches = w.findAll('[role="switch"]')
    expect(switches).toHaveLength(6)
    expect(switches.every((s) => s.attributes('disabled') !== undefined)).toBe(true)
    expect(w.get('#greet-follow').attributes('placeholder')).toBe('謝謝 {觀眾} 的追隨，歡迎！')
  })
})

describe('點歌', () => {
  it('shows nothing on and an empty queue, with the song actions disabled', async () => {
    const w = await page(SongsPage, '/songs')
    expect(w.get('h1').text()).toBe(t('pages.songs.name'))
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([t('songs.nothingNow'), t('songs.emptyQueue'), t('songs.boothOff')])
    for (const label of [t('songs.next'), t('songs.skip'), t('songs.prepare')])
      expect(button(w, label).attributes('disabled')).toBeDefined()
  })
})

describe('歌單', () => {
  it('shows no songs and no links, with reloading and finding links disabled', async () => {
    const w = await page(SongListPage, '/song-list')
    expect(w.get('h1').text()).toBe(t('pages.songList.name'))
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([t('songList.empty'), t('songList.noLinks')])
    for (const label of [t('songList.reload'), t('songList.findLinks')])
      expect(button(w, label).attributes('disabled')).toBeDefined()
  })
})

describe('練唱台', () => {
  it('shows where the booth will be, with opening it disabled', async () => {
    const w = await page(SingingBoothPage, '/singing-booth')
    expect(w.get('h1').text()).toBe(t('pages.singingBooth.name'))
    expect(w.get('.embed-body').text()).toBe(t('singingBooth.body'))
    expect(button(w, t('singingBooth.openOwn')).attributes('disabled')).toBeDefined()
  })
})

describe('SO', () => {
  it('shows no clip and no shoutouts, with the automatic shoutout switch disabled', async () => {
    const w = await page(ShoutoutsPage, '/shoutouts')
    expect(w.get('h1').text()).toBe(t('pages.shoutouts.name'))
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([t('shoutouts.nothingPlaying'), t('shoutouts.none')])
    expect(w.get('[role="switch"]').attributes('disabled')).toBeDefined()
  })
})

describe('揪團', () => {
  it('shows the raid form and the random pick, with every raid action disabled', async () => {
    const w = await page(RaidsPage, '/raids')
    expect(w.get('h1').text()).toBe(t('pages.raids.name'))
    expect(w.find('#raid-channel').exists()).toBe(true)
    expect(w.get('.empty-cell').text()).toBe(t('raids.randomHint'))
    for (const label of [t('raids.start'), t('raids.cancel'), t('raids.pick')])
      expect(button(w, label).attributes('disabled')).toBeDefined()
  })
})
