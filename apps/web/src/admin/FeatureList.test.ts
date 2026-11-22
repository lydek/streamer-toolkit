// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { describe, expect, it } from 'vitest'
import { i18n } from '../i18n'
import FeatureList from './FeatureList.vue'
import { testRouter } from './testRouter'

const list = () =>
  mount(FeatureList, {
    props: { modules: [{ id: 'chat', enabled: true, closed: false }], busy: false, loadText: '' },
    global: { plugins: [testRouter(), i18n, PrimeVue] },
  })

/** Each heading of a group's card with the feature names under it; '' for features with no heading. */
function sections(group: string, wrapper = list()) {
  const card = wrapper
    .findAll('section')
    .find((s) => s.get('h3').text().startsWith(group))!
  return card.findAll('h4, li.feat').reduce<[string, string[]][]>((out, el) => {
    if (el.element.tagName === 'H4') out.push([el.text(), []])
    else (out.at(-1) ?? out[out.push(['', []]) - 1])[1].push(el.get('.name').text())
    return out
  }, [])
}

describe('FeatureList', () => {
  it("lists a group's features under their categories, in the categories' order", () => {
    expect(sections('聊天')).toEqual([
      ['聊天室', ['聊天室', '聊天過濾']],
      ['聊天機器人', ['聊天機器人']],
    ])
    expect(sections('觀眾')[2]).toEqual(['揪團與 SO', ['SO', '感謝名單', '揪團']])
  })

  it('lists the features of a group without categories with no heading', () => {
    expect(sections('首頁')).toEqual([['', ['首頁', '直播資訊', '演練']]])
  })

  it('keeps only the matching features, under their categories, and drops groups left empty', async () => {
    const wrapper = list()
    await wrapper.get('input[type="search"]').setValue('過濾')
    expect(wrapper.findAll('section')).toHaveLength(1)
    expect(sections('聊天', wrapper)).toEqual([['聊天室', ['聊天過濾']]])
    await wrapper.get('input[type="search"]').setValue('')
    expect(sections('聊天', wrapper)).toHaveLength(2)
  })
})
