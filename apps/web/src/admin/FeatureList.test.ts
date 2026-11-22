// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { i18n } from '../i18n'
import FeatureList from './FeatureList.vue'
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
