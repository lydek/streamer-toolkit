// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { i18n } from '../i18n'
import TileFace from './TileFace.vue'

const face = (num: string) =>
  mount(TileFace, { props: { name: 'x', icon: '', tint: 'blue', line: { state: 'ready', num, text: '' }, rows: true }, global: { plugins: [i18n] } })

describe('TileFace', () => {
  it('shows counts and times at the count size, and words smaller', () => {
    for (const n of ['12', '1,234', '+3', '1:23', '2.5']) expect(face(n).get('.num').classes()).not.toContain('word')
    for (const n of ['遊戲畫面', '開著', 'Lo-fi']) expect(face(n).get('.num').classes()).toContain('word')
  })
})
