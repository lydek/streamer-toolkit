import { describe, expect, it } from 'vitest'
import { changesRecords, noFilters, pageCount, redemptionQuery } from './redemptions'

describe('redemptionQuery', () => {
  it('leaves empty filters out', () => {
    expect(redemptionQuery(noFilters())).toEqual({
      from: undefined,
      to: undefined,
      viewer: undefined,
      reward: undefined,
      status: undefined,
      offset: 0,
      limit: 50,
    })
    expect(redemptionQuery({ ...noFilters(), viewer: '  ', reward: ' ' }).viewer).toBeUndefined()
  })

  it('turns local dates into instants, with the end date included', () => {
    const q = redemptionQuery({ from: '2026-11-01', to: '2026-11-16', viewer: ' Ali ', reward: 'song', status: 'canceled' }, 2, 10)
    expect(q.from).toBe(new Date(2026, 10, 1).toISOString())
    // `to` is exclusive on the Host, so the whole last day is in.
    expect(q.to).toBe(new Date(2026, 10, 17).toISOString())
    expect(q).toMatchObject({ viewer: 'Ali', reward: 'song', status: 'canceled', offset: 20, limit: 10 })
  })

  it('crosses month ends', () => {
    expect(redemptionQuery({ ...noFilters(), to: '2026-11-30' }).to).toBe(new Date(2026, 11, 1).toISOString())
  })
})

describe('changesRecords', () => {
  it('reloads on record changes and on every snapshot', () => {
    const changed = {
      id: 'e',
      type: 'redemption.changed',
      version: 1,
      seq: 3,
      occurredAt: '',
      source: 'twitch',
      payload: { id: 'r' },
    } as const
    expect(changesRecords(changed)).toBe(true)
    expect(changesRecords({ type: 'snapshot', version: 1, seq: 0, payload: { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [] } })).toBe(true)
    expect(changesRecords({ ...changed, type: 'template.changed', payload: { overlay: 'chat' } })).toBe(false)
  })
})

it('pageCount has at least one page', () => {
  expect([pageCount(0), pageCount(50), pageCount(51)]).toEqual([1, 1, 2])
})
