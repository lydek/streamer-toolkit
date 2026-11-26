// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { i18n } from '../i18n'
import FeatureList from './FeatureList.vue'
import { twitchOverview } from './home'
import { useServices } from './services'
import { testRouter } from './testRouter'

const t = i18n.global.t

const list = () =>
  mount(FeatureList, {
    props: { modules: [{ id: 'chat', enabled: true, closed: false }], busy: false, loadText: '' },
    global: { plugins: [testRouter(), i18n, PrimeVue] },
  })

const name = (id: string) => t(`features.${id}.name`)
const category = (id: string) => t(`nav.categories.${id}`)

/** Each heading of a group's card with the feature names under it; '' for features with no heading. */
function sections(group: string, wrapper = list()) {
  const card = wrapper
    .findAll('section')
    .find((s) => s.get('h3').text().startsWith(t(`nav.groups.${group}`)))!
  return card.findAll('h4, li.feat').reduce<[string, string[]][]>((out, el) => {
    if (el.element.tagName === 'H4') out.push([el.text(), []])
    else (out.at(-1) ?? out[out.push(['', []]) - 1])[1].push(el.get('.name').text())
    return out
  }, [])
}

describe('FeatureList', () => {
  it("lists a group's features under their categories, in the categories' order", () => {
    expect(sections('chat')).toEqual([
      [category('chatroom'), [name('chat'), name('chatFilter')]],
      [category('bot'), [name('bot')]],
    ])
    expect(sections('viewers')[2]).toEqual([category('raids'), [name('shoutouts'), name('credits'), name('raids')]])
  })

  it("links a feature not built yet to its page in dev mode only", () => {
    const link = () => list().findAll('li.feat').find((li) => li.get('.name').text() === name('shoutouts'))!.find('a.open')
    expect(link().exists()).toBe(false)
    vi.stubEnv('MODE', 'development')
    try {
      expect(link().attributes('href')).toBe('/shoutouts')
      // Twitch not used: its pages are closed, so there is nothing to open.
      useServices().unused = ['twitch']
      expect(link().exists()).toBe(false)
    } finally {
      useServices().unused = []
      vi.unstubAllEnvs()
    }
  })

  it('lists the features of a group without categories with no heading', () => {
    expect(sections('home')).toEqual([['', [name('home'), name('streamInfo'), name('rehearsal')]]])
  })

  it('keeps only the matching features, under their categories, and drops groups left empty', async () => {
    const wrapper = list()
    // Part of the name, not all of it or its start: the search matches anywhere in it.
    await wrapper.get('input[type="search"]').setValue(name('chatFilter').slice(1))
    expect(wrapper.findAll('section')).toHaveLength(1)
    expect(sections('chat', wrapper)).toEqual([[category('chatroom'), [name('chatFilter')]]])
    await wrapper.get('input[type="search"]').setValue('')
    expect(sections('chat', wrapper)).toHaveLength(2)
  })

  it('gives the switch its focus back once the change it started has saved', async () => {
    const wrapper = mount(FeatureList, {
      props: { modules: [{ id: 'sound', enabled: true, closed: false }], busy: false, loadText: '' },
      global: { plugins: [testRouter(), i18n, PrimeVue] },
      attachTo: document.body,
    })
    await wrapper.get('#module-sound').trigger('change')
    expect(wrapper.emitted('toggle')).toHaveLength(1)
    await wrapper.setProps({ busy: true })
    ;(document.activeElement as HTMLElement | null)?.blur()
    await wrapper.setProps({ busy: false })
    await nextTick()
    expect(document.activeElement?.id).toBe('module-sound')
    wrapper.unmount()
  })
})

describe('FeatureList group switches', () => {
  beforeAll(() => {
    // jsdom has no modal dialogs; the real behavior is checked by hand (docs/dev.md).
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
      this.open = true
    }
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement, value?: string) {
      if (value !== undefined) this.returnValue = value
      this.open = false
      this.dispatchEvent(new Event('close'))
    }
  })
  const interact = ['alerts', 'checkin', 'redemption', 'counter', 'marathon', 'polls', 'rewards']
  const mounted = (on: string[]) =>
    mount(FeatureList, {
      props: { modules: interact.map((id) => ({ id, enabled: on.includes(id), closed: false })), busy: false, loadText: '' },
      global: { plugins: [testRouter(), i18n, PrimeVue] },
      attachTo: document.body,
    })
  const bulk = (wrapper: ReturnType<typeof mounted>, group: string, which: 'on' | 'off') =>
    wrapper.findAll('button').find((b) => b.attributes('aria-label') === t(`features.bulk.${which}Label`, { group: t(`nav.groups.${group}`) }))

  it('turns a whole group off after one confirmation listing it, and on again', async () => {
    const wrapper = mounted(['alerts', 'checkin', 'redemption'])
    // Only one built feature in 聊天: no group switches there.
    expect(bulk(wrapper, 'chat', 'off')).toBeUndefined()
    await bulk(wrapper, 'interact', 'off')!.trigger('click')
    expect([...document.querySelectorAll('.confirm li')].map((li) => li.textContent)).toEqual(
      ['alerts', 'checkin', 'redemption'].map(name),
    )
    ;[...document.querySelectorAll<HTMLButtonElement>('.confirm button')].find((b) => b.textContent?.includes(t('features.bulk.off')))!.click()
    await nextTick()
    expect(wrapper.emitted('bulk')![0][0]).toEqual(['alerts', 'checkin', 'redemption'].map((id) => ({ id, enabled: false })))
    await bulk(wrapper, 'interact', 'on')!.trigger('click')
    ;[...document.querySelectorAll<HTMLButtonElement>('.confirm button')].find((b) => b.textContent?.includes(t('features.bulk.on')))!.click()
    await nextTick()
    expect(wrapper.emitted('bulk')![1][0]).toEqual(['counters', 'marathon'].map((f) => ({ id: f === 'counters' ? 'counter' : f, enabled: true })))
    wrapper.unmount()
  })

  it('turning on a feature without its Twitch scopes says which, then asks to sign in again for them', async () => {
    twitchOverview.value = { client_id: 'cid', eventsub_connected: true, irc_connected: false, scopes: ['user:read:chat'], status: { state: 'signed_in', user_id: '1', login: 'me' } }
    const wrapper = mount(FeatureList, {
      props: { modules: [{ id: 'stream-info', enabled: false, closed: false }], busy: false, loadText: '' },
      global: { plugins: [testRouter(), i18n, PrimeVue] },
      attachTo: document.body,
    })
    await wrapper.get('#module-stream-info').trigger('change')
    expect(document.querySelector('.confirm h2')?.textContent).toBe(t('features.scopes.title', { name: name('streamInfo') }))
    expect([...document.querySelectorAll('.confirm li')].map((li) => li.textContent)).toEqual(['channel:manage:broadcast'])
    expect(wrapper.emitted('toggle')).toBeUndefined()
    ;[...document.querySelectorAll<HTMLButtonElement>('.confirm button')].find((b) => b.textContent?.includes(t('features.scopes.ok')))!.click()
    await nextTick()
    // Not turned on yet: the page does that once the sign-in grants the scope.
    expect(wrapper.emitted('toggle')).toBeUndefined()
    expect(wrapper.emitted('scopes')![0]).toEqual([['channel:manage:broadcast'], [{ id: 'stream-info', enabled: true }]])
    // Already granted: it just turns on.
    twitchOverview.value = { ...twitchOverview.value, scopes: ['channel:manage:broadcast'] }
    await wrapper.get('#module-stream-info').trigger('change')
    expect(wrapper.emitted('toggle')).toHaveLength(1)
    expect(wrapper.emitted('scopes')).toHaveLength(1)
    twitchOverview.value = undefined
    wrapper.unmount()
  })

  it('turning a group on leaves the features still needing Twitch scopes off, and says so', async () => {
    twitchOverview.value = { client_id: 'cid', eventsub_connected: true, irc_connected: false, scopes: [], status: { state: 'signed_in', user_id: '1', login: 'me' } }
    try {
      // 投票與預測 and 頻道點數獎勵管理 need scopes the sign-in lacks.
      const wrapper = mounted(['alerts', 'checkin', 'redemption', 'marathon'])
      await bulk(wrapper, 'interact', 'on')!.trigger('click')
      expect([...document.querySelectorAll('.confirm li')].map((li) => li.textContent)).toEqual([
        name('counters'),
        t('features.scopes.skipped', { name: name('polls') }),
        t('features.scopes.skipped', { name: name('rewards') }),
      ])
      ;[...document.querySelectorAll<HTMLButtonElement>('.confirm button')].find((b) => b.textContent?.includes(t('features.bulk.on')))!.click()
      await nextTick()
      expect(wrapper.emitted('bulk')![0][0]).toEqual([{ id: 'counter', enabled: true }])
      wrapper.unmount()
    } finally {
      twitchOverview.value = undefined
    }
  })

  it('sends nothing when the confirmation is cancelled, and disables a switch with nothing to do', async () => {
    const wrapper = mounted(interact)
    expect(bulk(wrapper, 'interact', 'on')!.attributes('disabled')).toBeDefined()
    await bulk(wrapper, 'interact', 'off')!.trigger('click')
    ;[...document.querySelectorAll<HTMLButtonElement>('.confirm button')].find((b) => b.textContent?.includes(t('features.cancel')))!.click()
    await nextTick()
    expect(wrapper.emitted('bulk')).toBeUndefined()
    wrapper.unmount()
  })

  it('shows a service switch on again when not using it is cancelled', async () => {
    const wrapper = mounted(interact)
    const use = () => wrapper.get<HTMLInputElement>('#use-twitch')
    await use().setValue(false)
    expect(document.querySelector('.confirm')?.hasAttribute('open')).toBe(true)
    ;[...document.querySelectorAll<HTMLButtonElement>('.confirm button')].find((b) => b.textContent?.includes(t('features.cancel')))!.click()
    await nextTick()
    expect(wrapper.emitted('service')).toBeUndefined()
    expect(use().element.checked).toBe(true)
    wrapper.unmount()
  })
})
