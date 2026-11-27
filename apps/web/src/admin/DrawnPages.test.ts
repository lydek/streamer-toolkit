// @vitest-environment jsdom
// Pages drawn ahead of their backend (UI-G): they render the prototype's layout empty, with backend actions disabled.
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import type { Component } from 'vue'
import { i18n } from '../i18n'
import SongsPage from './SongsPage.vue'
import SingingBoothPage from './SingingBoothPage.vue'
import ShoutoutsPage from './ShoutoutsPage.vue'
import RaidsPage from './RaidsPage.vue'
import CreditsPage from './CreditsPage.vue'
import ModesPage from './ModesPage.vue'
import ViewersPage from './ViewersPage.vue'
import RolesPage from './RolesPage.vue'
import ChatFilterPage from './ChatFilterPage.vue'
import AvatarPage from './AvatarPage.vue'
import CaptionsPage from './CaptionsPage.vue'
import GoalsPage from './GoalsPage.vue'
import MusicRequestsPage from './MusicRequestsPage.vue'
import ChatWindowPage from './ChatWindowPage.vue'
import SettingsHotkeys from './SettingsHotkeys.vue'
import SettingsProfiles from './SettingsProfiles.vue'
import MicAvatarPage from './MicAvatarPage.vue'
import OutfitsPage from './OutfitsPage.vue'
import SongRulesPage from './SongRulesPage.vue'
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

describe('點歌', () => {
  it('shows nothing on and an empty queue, with the song actions disabled', async () => {
    const w = await page(SongsPage, '/songs')
    expect(w.get('h1').text()).toBe(t('pages.songs.name'))
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([t('songs.nothingNow'), t('songs.emptyQueue'), t('songs.boothOff')])
    for (const label of [t('songs.next'), t('songs.skip'), t('songs.prepare')])
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

describe('感謝名單', () => {
  it('shows every kind empty, with playing the lists disabled', async () => {
    const w = await page(CreditsPage, '/credits')
    expect(w.get('h1').text()).toBe(t('pages.credits.name'))
    expect(w.findAll('.empty-cell')).toHaveLength(7)
    for (const label of [t('credits.playThanks'), t('credits.playEnd'), t('credits.manageBackgrounds')])
      expect(button(w, label).attributes('disabled')).toBeDefined()
  })
})

describe('模式', () => {
  it('shows no modes, with creating one disabled', async () => {
    const w = await page(ModesPage, '/modes')
    expect(w.get('h1').text()).toBe(t('pages.modes.name'))
    expect(w.get('.empty-cell').text()).toBe(t('modes.empty'))
    expect(button(w, t('modes.fromObs')).attributes('disabled')).toBeDefined()
  })
})

describe('觀眾清單', () => {
  it('shows the search with no viewers yet, with syncing and searching disabled', async () => {
    const w = await page(ViewersPage, '/viewers')
    expect(w.get('h1').text()).toBe(t('pages.viewers.name'))
    expect(w.get('.empty-cell').text()).toBe(t('viewers.empty'))
    for (const label of [t('viewers.sync'), t('viewers.history'), t('viewers.searchButton')])
      expect(button(w, label).attributes('disabled')).toBeDefined()
  })

  it('shows no custom roles, with adding one disabled', async () => {
    const w = await page(RolesPage, '/roles')
    expect(w.get('h1').text()).toBe(t('pages.roles.name'))
    expect(w.get('.empty-cell').text()).toBe(t('viewers.groups.empty'))
    expect(button(w, t('viewers.groups.add')).attributes('disabled')).toBeDefined()
  })
})

describe('聊天過濾', () => {
  it('shows one row per rule with its switch disabled, and nothing caught yet', async () => {
    const w = await page(ChatFilterPage, '/chat-filter')
    expect(w.get('h1').text()).toBe(t('pages.chatFilter.name'))
    const switches = w.findAll('[role="switch"]')
    expect(switches).toHaveLength(6)
    expect(switches.every((s) => s.attributes('disabled') !== undefined)).toBe(true)
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([
      t('chatFilter.noTerms'),
      t('chatFilter.noUrls'),
      t('chatFilter.noRecent'),
    ])
    expect(button(w, t('chatFilter.addUrlButton')).attributes('disabled')).toBeDefined()
    // The prototype's thresholds, and a timeout length only on the rules that time out (repeats, characters).
    expect(w.find(`[aria-label="${t('chatFilter.limits.repeats.seconds')}"]`).exists()).toBe(true)
    expect(w.findAll(`[aria-label$="${t('chatFilter.duration', { name: '' })}"]`)).toHaveLength(2)
  })
})

describe('虛擬形象', () => {
  it('3D: shows both apps not connected, the faces and setups disabled, and no interactions', async () => {
    const w = await page(AvatarPage, '/avatar')
    expect(w.get('h1').text()).toBe(t('pages.avatar.name'))
    expect(w.findAll('.empty-cell').map((e) => e.text())).toEqual([
      t('avatar.notConnected'),
      t('avatar.notConnected'),
      t('avatar.noItems'),
    ])
    expect(button(w, t('avatar.face.joy')).attributes('disabled')).toBeDefined()
  })

  it('跳跳: shows the mic settings with the filter switch disabled and no image sets', async () => {
    const w = await page(MicAvatarPage, '/mic-avatar')
    expect(w.get('h1').text()).toBe(t('pages.micAvatar.name'))
    expect(w.get('.empty-cell').text()).toBe(t('avatar.noSets'))
    expect(w.get('[role="switch"]').attributes('disabled')).toBeDefined()
  })

  it('換裝: shows no outfits and no reward to pick', async () => {
    const w = await page(OutfitsPage, '/outfits')
    expect(w.get('h1').text()).toBe(t('pages.outfits.name'))
    expect(w.text()).toContain(t('avatar.outfits.none'))
    expect(w.find('#av-reward').exists()).toBe(true)
  })
})

describe('字幕與目標', () => {
  it('shows the caption settings with their switches disabled', async () => {
    const w = await page(CaptionsPage, '/captions')
    expect(w.get('h1').text()).toBe(t('pages.captions.name'))
    const switches = w.findAll('[role="switch"]')
    expect(switches).toHaveLength(2)
    expect(switches.every((s) => s.attributes('disabled') !== undefined)).toBe(true)
  })

  it('shows no goals', async () => {
    const w = await page(GoalsPage, '/goals')
    expect(w.get('h1').text()).toBe(t('pages.goals.name'))
    expect(w.get('.empty-cell').text()).toBe(t('goals.empty'))
  })
})

describe('YouTube 點播', () => {
  it('shows no requests, with opening requests and the settings disabled', async () => {
    const w = await page(MusicRequestsPage, '/music-requests')
    expect(w.get('h1').text()).toBe(t('pages.musicRequests.name'))
    expect(w.get('.empty-cell').text()).toBe(t('musicRequests.empty'))
    expect(w.get('#mr-open').attributes('disabled')).toBeDefined()
    expect(button(w, t('musicRequests.settings')).attributes('disabled')).toBeDefined()
  })
})

describe('桌面置頂聊天室', () => {
  it('shows the window settings, with opening it and click-through disabled', async () => {
    const w = await page(ChatWindowPage, '/chat-window')
    expect(w.get('h1').text()).toBe(t('pages.chatWindow.name'))
    expect(button(w, t('chatWindow.open')).attributes('disabled')).toBeDefined()
    expect(w.get('[role="switch"]').attributes('disabled')).toBeDefined()
    expect(w.findAll('[role="group"] input[type="checkbox"]')).toHaveLength(4)
  })
})

describe('設定 › 設定檔與快捷鍵', () => {
  it('shows no other profiles and no hotkeys, with adding them and global hotkeys disabled', async () => {
    const profiles = await page(SettingsProfiles, '/settings/profiles')
    expect(profiles.get('.empty-cell').text()).toBe(t('profiles.empty'))
    expect(button(profiles, t('profiles.add')).attributes('disabled')).toBeDefined()
    const hotkeys = await page(SettingsHotkeys, '/settings/hotkeys')
    expect(hotkeys.get('[role="switch"]').attributes('disabled')).toBeDefined()
    expect(button(hotkeys, t('hotkeys.add')).attributes('disabled')).toBeDefined()
  })
})

describe('點歌規則', () => {
  it('shows the request rules with their switches disabled', async () => {
    const w = await page(SongRulesPage, '/song-rules')
    expect(w.get('h1').text()).toBe(t('pages.songRules.name'))
    const switches = w.findAll('[role="switch"]')
    expect(switches).toHaveLength(3)
    expect(switches.every((s) => s.attributes('disabled') !== undefined)).toBe(true)
    expect(button(w, t('songRules.change')).attributes('disabled')).toBeDefined()
  })
})
