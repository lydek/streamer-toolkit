// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import type { RouteLocationNormalized } from 'vue-router'
import { focusReturnTarget, keepFocus, openerSelector, rememberOpener } from './page'

const route = (path: string) => ({ path, matched: [{}] }) as unknown as RouteLocationNormalized
const home = route('/')
const settings = route('/settings')

function homeDom(tiles: boolean) {
  document.body.innerHTML = `
    <button data-page-opener="settings">⚙</button>
    ${tiles ? '<button data-drawer-tile="obs">obs</button><button data-drawer-tile="chat">chat</button>' : ''}
    <button data-drawer-arrange>arrange</button>`
}
const q = (selector: string) => document.querySelector<HTMLElement>(selector)!

afterEach(() => (document.body.innerHTML = ''))

describe('openerSelector', () => {
  it('finds tiles, page openers and "Arrange tiles" again', () => {
    homeDom(true)
    expect(openerSelector(q('[data-drawer-tile="chat"]'))).toBe('[data-drawer-tile="chat"]')
    expect(openerSelector(q('[data-page-opener]'))).toBe('[data-page-opener="settings"]')
    expect(openerSelector(q('[data-drawer-arrange]'))).toBe('[data-drawer-arrange=""]')
    expect(openerSelector(document.body)).toBeNull()
    expect(openerSelector(null)).toBeNull()
  })

  it('escapes quotes in attribute values', () => {
    const el = document.createElement('button')
    el.setAttribute('data-drawer-tile', 'a"b')
    document.body.append(el)
    expect(document.querySelector(openerSelector(el)!)).toBe(el)
  })
})

describe('returning to home', () => {
  // Home re-renders after the page, so every check builds a fresh DOM before focusReturnTarget.
  it('focuses the re-rendered opener', async () => {
    homeDom(true)
    rememberOpener(settings, home, q('[data-page-opener]'))
    homeDom(true)
    await focusReturnTarget()
    expect(document.activeElement).toBe(q('[data-page-opener]'))
  })

  it("prefers ⚙ beside an open drawer over the tile the drawer's close focused", async () => {
    homeDom(true)
    const cog = q('[data-page-opener]')
    q('[data-drawer-tile="chat"]').focus()
    rememberOpener(settings, home, cog)
    homeDom(true)
    await focusReturnTarget()
    expect(document.activeElement).toBe(q('[data-page-opener]'))
  })

  it('takes the tile for a drawer\'s "Open full page" link', async () => {
    homeDom(true)
    const link = document.createElement('a')
    q('[data-drawer-tile="chat"]').focus() // closing the drawer focused its tile
    rememberOpener(settings, home, link)
    homeDom(true)
    await focusReturnTarget()
    expect(document.activeElement).toBe(q('[data-drawer-tile="chat"]'))
  })

  it('falls back to the first tile, then "Arrange tiles"', async () => {
    homeDom(false)
    rememberOpener(settings, home, q('[data-drawer-arrange]'))
    homeDom(true) // a module was turned on, so "Arrange tiles" is gone
    q('[data-drawer-arrange]').remove()
    await focusReturnTarget()
    expect(document.activeElement).toBe(q('[data-drawer-tile="obs"]'))

    homeDom(false)
    rememberOpener(settings, home, null)
    homeDom(false)
    await focusReturnTarget()
    expect(document.activeElement).toBe(q('[data-drawer-arrange]'))
  })

  it('moves focus only when home was left for a page', async () => {
    homeDom(true)
    rememberOpener(home, settings, q('[data-page-opener]')) // page to home
    rememberOpener(home, { path: '/', matched: [] } as unknown as RouteLocationNormalized, null) // app start
    await focusReturnTarget()
    expect(document.activeElement).toBe(document.body)
  })
})

describe('keepFocus', () => {
  afterEach(() => (document.body.innerHTML = ''))

  it('moves focus on only when the focused control went away', async () => {
    document.body.innerHTML = '<button id="gone">x</button><button id="kept">y</button><input id="target">'
    const target = () => document.getElementById('target')
    document.getElementById('kept')!.focus()
    await keepFocus(target)
    expect(document.activeElement?.id).toBe('kept')

    document.getElementById('gone')!.focus()
    document.getElementById('gone')!.remove()
    await keepFocus(target)
    expect(document.activeElement?.id).toBe('target')
  })
})
