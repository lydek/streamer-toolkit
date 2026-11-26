import assert from 'node:assert/strict'
import { test } from 'node:test'
import { SCENARIOS, markdown, match, percentile, resources } from './bench.mjs'

test('percentiles use the nearest rank', () => {
  const values = [5, 1, 4, 2, 3, 10, 9, 8, 7, 6]
  assert.equal(percentile(values, 50), 5)
  assert.equal(percentile(values, 95), 10)
  assert.equal(percentile(values, 100), 10)
  assert.equal(percentile([], 50), null)
})

test('a message the overlay never got is reported missing', () => {
  const sent = new Map([['a', 0], ['b', 10], ['c', 20]])
  const received = new Map([['a', 5], ['c', 50]])
  const m = match(sent, received)
  assert.equal(m.sent, 3)
  assert.equal(m.received, 2)
  assert.deepEqual(m.missing, ['b'])
  assert.deepEqual(m.latency_ms, { p50: 5, p95: 30, max: 30 })
  assert.match(markdown(report({ chat: m })), /\*\*Missing:\*\* b/)
})

test('resources take peaks and averages of the samples that have them', () => {
  const r = resources([{ app_cpu_percent: 2, app_memory_mb: 40 }, {}, { app_cpu_percent: 4, app_memory_mb: 50 }])
  assert.deepEqual(r, { samples: 3, app_cpu_percent: { peak: 4, avg: 3 }, app_memory_mb: { peak: 50, avg: 45 } })
})

test('the report has every section and flags a redemption mismatch', () => {
  const md = markdown(report({ redemptions: { sent: 3, recorded: 2 } }))
  for (const part of ['# Load run: chat', '## Overlay', '## Redemptions', '**(mismatch)**', '## App resources', '## API calls', 'GET /helix/users', '## Warnings', 'duplicate']) {
    assert.ok(md.includes(part), part)
  }
  assert.ok(Object.keys(SCENARIOS).includes('idle'))
})

function report(overrides) {
  const empty = { sent: 0, received: 0, missing: [], latency_ms: { p50: null, p95: null, max: null } }
  return {
    scenario: 'chat',
    started_at: '2026-11-26T00:00:00.000Z',
    duration_s: 20,
    drain_s: 0.4,
    chat: empty,
    alerts: { ...empty, drain_ms: null },
    redemptions: { sent: 0, recorded: 0 },
    resources: resources([]),
    api: {
      calls: [{ key: 'GET /helix/users', total: 3, per_minute: 9, peak_10s: 3, min_gap_ms: 100 }],
      helix: { total: 3, peak_per_minute: 3, bucket_percent: 0.4 },
      warnings: [{ at_s: 1, kind: 'duplicate', key: 'GET /helix/users', detail: 'x' }],
    },
    ...overrides,
  }
}
