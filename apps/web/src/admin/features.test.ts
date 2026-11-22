import { describe, expect, it } from 'vitest'
import { FEATURES, type Feature, hiddenPages, isOn, turnOffPlan, turnOnPlan } from './features'
import { GROUPS } from './tabs'

const mod = (id: string, enabled: boolean) => ({ id, enabled, closed: false })

describe('FEATURES', () => {
  it('owns every tab page exactly once, in the group the tab is in', () => {
    const owner = new Map<string, Feature>()
    for (const f of FEATURES) for (const p of f.pages) {
      expect(owner.has(p), p).toBe(false)
      owner.set(p, f)
    }
    for (const g of GROUPS) for (const p of g.pages) expect(owner.get(p.path)?.group, p.path).toBe(g.id)
    expect(owner.size).toBe(GROUPS.flatMap((g) => g.pages).length)
  })

  it('only depends on features that exist', () => {
    const ids = new Set(FEATURES.map((f) => f.id))
    for (const f of FEATURES) for (const d of [...(f.requires ?? []), ...(f.affects ?? [])]) expect(ids.has(d), `${f.id} → ${d}`).toBe(true)
  })
})

describe('isOn', () => {
  const chat = FEATURES.find((f) => f.id === 'chat')!
  it('follows the module of a built feature', () => {
    expect(isOn(chat, [mod('chat', true)])).toBe(true)
    expect(isOn(chat, [mod('chat', false)])).toBe(false)
  })
  it('is always on for home and never on for a feature not built yet', () => {
    expect(isOn(FEATURES.find((f) => f.id === 'home')!, [])).toBe(true)
    expect(isOn(FEATURES.find((f) => f.id === 'songs')!, [])).toBe(false)
  })
})

// Fixture features: no built feature has dependencies or scopes yet.
const A: Feature = { id: 'a', group: 'g', icon: '', pages: ['/a'], scopes: ['x:read', 'x:write'] }
const B: Feature = { id: 'b', group: 'g', icon: '', pages: ['/b'], requires: ['a'] }
const C: Feature = { id: 'c', group: 'g', icon: '', pages: ['/c'], affects: ['b'] }
const ALL = [A, B, C]

describe('turnOnPlan', () => {
  it('asks to turn on what it requires, and for scopes not granted yet', () => {
    const plan = turnOnPlan(B, () => false, new Set(), ALL)
    expect(plan.requires.map((f) => f.id)).toEqual(['a'])
    expect(turnOnPlan(A, () => false, new Set(['x:read']), ALL).scopes).toEqual(['x:write'])
  })
  it('needs nothing once requirements are on and scopes granted', () => {
    expect(turnOnPlan(B, (f) => f.id === 'a', new Set(), ALL)).toEqual({ requires: [], scopes: [] })
    expect(turnOnPlan(A, () => false, new Set(['x:read', 'x:write']), ALL).scopes).toEqual([])
  })
})

describe('turnOffPlan', () => {
  it('takes along the features that require it and lists the ones it affects', () => {
    expect(turnOffPlan(A, () => true, ALL).dependents.map((f) => f.id)).toEqual(['b'])
    expect(turnOffPlan(C, () => true, ALL).affected.map((f) => f.id)).toEqual(['b'])
  })
  it('ignores features that are already off', () => {
    expect(turnOffPlan(A, (f) => f.id !== 'b', ALL)).toEqual({ dependents: [], affected: [] })
  })
})

describe('hiddenPages', () => {
  it('hides the pages of built features that are off, never unbuilt ones', () => {
    const hidden = hiddenPages([mod('chat', false), mod('alerts', true)])
    expect([...hidden].sort()).toEqual(['/chat', '/chat-log', '/chat-window'])
  })
  it('hides nothing for modules not listed, e.g. before they load', () => {
    expect(hiddenPages([]).size).toBe(0)
  })
})

describe('hiddenPages', () => {
  it('keeps 兌換時播放 while sounds or media are on, and hides it once both are off', () => {
    const states = (off: string[]) => ['sound', 'media'].map((id) => ({ id, enabled: !off.includes(id), closed: false }))
    expect(hiddenPages(states(['sound'])).has('/reward-playback')).toBe(false)
    expect(hiddenPages(states(['media'])).has('/reward-playback')).toBe(false)
    expect(hiddenPages(states(['sound', 'media'])).has('/reward-playback')).toBe(true)
    expect(hiddenPages(states(['sound'])).has('/sounds')).toBe(true)
  })
})
