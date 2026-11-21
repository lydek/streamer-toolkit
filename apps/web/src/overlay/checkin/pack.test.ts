// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { fillPack, packOf, parsePack } from './pack'

const BASE = 'http://127.0.0.1:47810/templates/checkin/0190a1b2-0000-7000-8000-000000000001/'
const HTML = `<link rel="stylesheet" href="style.css"><div data-card-stage></div>
<template data-card><div class="card"><img data-field="avatar"><b data-field="title" onclick="x()"></b>
<span data-field="count"></span><ol data-field="slots"></ol><em data-field="banner">滿了</em></div></template>
<script>alert(1)</script>`

const payload = (over: object = {}) => ({
  userName: '<b>Ann</b>',
  avatarUrl: 'https://evil.example/a.png',
  stamps: 3,
  slots: 5,
  total: 13,
  full: false,
  durationMs: 5000,
  sound: null,
  title: '<b>Ann</b> 打卡',
  description: '累計 13 次',
  style: { preset: 'pack:x', backgroundColor: '#000000', backgroundUrl: null, stampColor: '#ff0000', stampUrl: null, perRow: 5, animation: 'none' },
  ...over,
})

describe('animation packs', () => {
  it('names the pack only for a well-formed pack preset', () => {
    expect(packOf('pack:0190a1b2-0000-7000-8000-000000000001')).toBe('0190a1b2-0000-7000-8000-000000000001')
    expect(packOf('simple')).toBeUndefined()
    expect(packOf('pack:../x')).toBeUndefined()
  })

  it('mounts no script or handler and fills viewer text as text', () => {
    const pack = parsePack(HTML, BASE, '1')!
    const host = document.createElement('div')
    host.append(...pack.nodes)
    expect(host.querySelector('script')).toBeNull()
    expect((host.querySelector('link') as HTMLLinkElement).href).toBe(`${BASE}style.css?v=1`)
    pack.stage.replaceChildren(fillPack(pack.item, payload()))
    const card = host.querySelector('.card') as HTMLElement
    expect(card.querySelector('[data-field="title"]')!.textContent).toBe('<b>Ann</b> 打卡')
    expect(card.querySelector('[data-field="title"] b')).toBeNull()
    expect(card.querySelector('[data-field="title"]')!.hasAttribute('onclick')).toBe(false)
    expect(card.querySelector('img')).toBeNull()
    expect(card.querySelectorAll('li.stamped')).toHaveLength(3)
    expect(card.querySelector('[data-field="banner"]')).toBeNull()
    expect(card.style.getPropertyValue('--stamp')).toBe('#ff0000')
  })

  it('needs its markers', () => {
    expect(parsePack('<div>no markers</div>', BASE, '1')).toBeNull()
  })
})
