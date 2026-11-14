// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import PrimeVue from 'primevue/config'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, h, nextTick, ref } from 'vue'
import { i18n } from '../i18n'
import AppDrawer from './AppDrawer.vue'
import { focusTarget, onDrawerSubmit, useDrawer, useDrawerSetting } from './drawer'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const drawer = useDrawer()
let cleanups: (() => void)[] = []

function submit(fn: () => Promise<boolean>) {
  cleanups.push(onDrawerSubmit(fn))
}

beforeEach(() => {
  document.body.innerHTML = `
    <button id="opener">opener</button>
    <button data-drawer-tile="obs">obs</button>
    <button data-drawer-tile="chat">chat</button>
    <button data-drawer-arrange>arrange</button>`
})

afterEach(async () => {
  cleanups.forEach((c) => c())
  cleanups = []
  await drawer.close()
})

const byId = (id: string) => document.getElementById(id)!
const tile = (id: string) => document.querySelector<HTMLElement>(`[data-drawer-tile="${id}"]`)!

describe('focusTarget', () => {
  it('prefers the opener', () => {
    expect(focusTarget(document, 'chat', byId('opener'))).toBe(byId('opener'))
  })

  it('falls back to the tile, the first tile, then "Arrange tiles"', () => {
    const removed = document.createElement('button')
    expect(focusTarget(document, 'chat', removed)).toBe(tile('chat'))
    expect(focusTarget(document, 'chat', null)).toBe(tile('chat'))
    tile('chat').remove()
    expect(focusTarget(document, 'chat', null)).toBe(tile('obs'))
    tile('obs').setAttribute('disabled', '')
    expect(focusTarget(document, 'chat', null)).toBe(document.querySelector('[data-drawer-arrange]'))
  })
})

describe('useDrawer', () => {
  it('submits changed settings before closing, then returns focus to the opener', async () => {
    const seen: string[] = []
    submit(async () => (seen.push(`a:${drawer.current.value}`), true))
    submit(async () => (seen.push(`b:${drawer.current.value}`), true))
    await drawer.open('chat', byId('opener'))

    expect(await drawer.close()).toBe(true)
    expect(seen).toEqual(['a:chat', 'b:chat'])
    expect(drawer.current.value).toBeNull()
    expect(document.activeElement).toBe(byId('opener'))
  })

  it('stays open when a submit fails, after trying every submit', async () => {
    const later = vi.fn(async () => true)
    submit(async () => false)
    submit(later)
    await drawer.open('chat')

    expect(await drawer.close()).toBe(false)
    expect(later).toHaveBeenCalled()
    expect(drawer.current.value).toBe('chat')
  })

  it('submits before switching to another drawer', async () => {
    let ok = false
    submit(async () => ok)
    await drawer.open('chat')

    expect(await drawer.open('obs')).toBe(false)
    expect(drawer.current.value).toBe('chat')
    ok = true
    expect(await drawer.open('obs')).toBe(true)
    expect(drawer.current.value).toBe('obs')
  })

  it('keeps the drawer when a second close comes in while a failing one is pending', async () => {
    submit(async () => false)
    await drawer.open('chat')
    expect(await Promise.all([drawer.close(), drawer.close()])).toEqual([false, false])
    expect(drawer.current.value).toBe('chat')
  })

  it('shares a pending close instead of submitting twice', async () => {
    let release!: () => void
    const save = vi.fn(() => new Promise<boolean>((r) => (release = () => r(true))))
    submit(save)
    await drawer.open('chat')

    const first = drawer.close()
    const second = drawer.close()
    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(1))
    release()
    expect(await first).toBe(true)
    expect(await second).toBe(true)
    expect(save).toHaveBeenCalledTimes(1)
  })
})

describe('useDrawerSetting', () => {
  function setting(save: (v: string) => Promise<unknown>) {
    const saved = ref('old')
    const scope = effectScope()
    const s = scope.run(() => useDrawerSetting(saved, save))!
    cleanups.push(() => scope.stop())
    return { saved, ...s }
  }

  it('saves only a changed value, once even when committed twice at once', async () => {
    const save = vi.fn(async () => {})
    const s = setting(save)
    expect(await s.commit()).toBe(true)
    expect(save).not.toHaveBeenCalled()

    s.value.value = 'new'
    const [a, b] = await Promise.all([s.commit(), s.commit()])
    expect([a, b]).toEqual([true, true])
    expect(save).toHaveBeenCalledTimes(1)
    expect(s.saved.value).toBe('new')
  })

  it('restores the saved value and shows the error when the save fails', async () => {
    const s = setting(async () => {
      throw new Error('boom')
    })
    s.value.value = 'new'
    expect(await s.commit()).toBe(false)
    expect(s.value.value).toBe('old')
    expect(drawer.error.value).toContain('boom')
  })

  it('keeps the drawer open when a blur commit failed just before ✕', async () => {
    const s = setting(async () => {
      throw new Error('boom')
    })
    await drawer.open('chat')
    s.value.value = 'new'
    void s.commit() // blur on the way to the close button; nobody awaits it

    expect(await drawer.close()).toBe(false)
    expect(drawer.current.value).toBe('chat')
    expect(drawer.error.value).toContain('boom')
    expect(await drawer.close()).toBe(true) // the user saw the error and closes again
  })

  it("keeps one setting's error when another one saves", async () => {
    const failing = setting(async () => {
      throw new Error('boom')
    })
    const fine = setting(async () => {})
    failing.value.value = 'new'
    await failing.commit()
    fine.value.value = 'new'
    await fine.commit()
    expect(drawer.error.value).toContain('boom')
  })
})

describe('AppDrawer (side by side)', () => {
  beforeEach(() => {
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  })

  it('focuses its close button on open and closes on Esc back to the opener', async () => {
    const wrapper = mount(AppDrawer, {
      props: { id: 'chat', title: 'Chat' },
      slots: { now: () => h('p', 'now') },
      global: { plugins: [i18n, PrimeVue] },
      attachTo: document.body,
    })
    const aside = wrapper.get('aside').element as HTMLElement
    expect(aside.hidden).toBe(true)

    await drawer.open('chat', byId('opener'))
    await nextTick()
    expect(aside.hidden).toBe(false)
    expect(document.activeElement).toBe(aside.querySelector('[data-drawer-close]'))

    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await vi.waitFor(() => expect(drawer.current.value).toBeNull())
    expect(document.activeElement).toBe(byId('opener'))
    wrapper.unmount()
  })
})
