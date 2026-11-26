// `node scripts/bench.mjs <scenario> --host 127.0.0.1:<host port> --fake 127.0.0.1:<fake Twitch port> [--duration <s>]`:
// a load run against an `e2e_host` in fake-services mode (docs/dev.md). It starts nothing itself: it drives the fakes
// through `/_control/…`, listens on the Host's read-only `/ws/overlay` and polls `/api/admin/performance` with
// `E2E_ADMIN_TOKEN`, then writes `target/bench/bench-<scenario>-<time>.md` and `.json`. Never a CI check.
import { mkdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(import.meta.dirname, '..')

export const SCENARIOS = {
  idle: { what: 'no events, to see how often the app calls the APIs on its own', duration: 600 },
  chat: { what: '50 chat messages a second', duration: 20 },
  gifts: { what: 'one viewer gifts 100 subs at once', duration: 10 },
  redemptions: { what: '200 channel point redemptions, 20 a second', duration: 12 },
  obs: { what: '30 extra audio meters every 50 ms, 20 scene switches and 20 sources added and removed', duration: 20 },
  all: { what: 'chat, redemptions and the OBS load at the same time', duration: 20 },
}

/** The value at percentile `p` (0–100) of `values`, nearest rank; `null` for none. */
export function percentile(values, p) {
  if (values.length === 0) return null
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))]
}

/** Matches sent IDs (with send times) to received ones (with arrival times): counts, missing IDs and latency. */
export function match(sent, received) {
  const latencies = []
  const missing = []
  for (const [id, at] of sent) {
    const arrived = received.get(id)
    if (arrived === undefined) missing.push(id)
    else latencies.push(arrived - at)
  }
  const round = (v) => (v === null ? null : Math.round(v * 10) / 10)
  return {
    sent: sent.size,
    received: sent.size - missing.length,
    missing,
    latency_ms: { p50: round(percentile(latencies, 50)), p95: round(percentile(latencies, 95)), max: round(percentile(latencies, 100)) },
  }
}

/** Peak and average of the app's CPU and memory over the performance samples. */
export function resources(samples) {
  const pick = (k) => samples.map((s) => s[k]).filter((v) => typeof v === 'number')
  const stats = (values) =>
    values.length === 0
      ? { peak: null, avg: null }
      : { peak: Math.max(...values), avg: Math.round((values.reduce((a, b) => a + b, 0) / values.length) * 10) / 10 }
  return { samples: samples.length, app_cpu_percent: stats(pick('app_cpu_percent')), app_memory_mb: stats(pick('app_memory_mb')) }
}

const cell = (v) => (v === null || v === undefined ? '-' : String(v))

/** The report as Markdown. */
export function markdown(r) {
  const lines = [`# Load run: ${r.scenario}`, '', `${SCENARIOS[r.scenario]?.what ?? ''}`, '']
  lines.push(`- Started: ${r.started_at}`, `- Load: ${r.duration_s} s, drained in ${r.drain_s} s`, '')
  lines.push('## Overlay', '', '| | sent | received | missing | p50 ms | p95 ms | max ms |', '|---|---|---|---|---|---|---|')
  for (const [name, m] of [['chat', r.chat], ['alerts', r.alerts]]) {
    lines.push(`| ${name} | ${m.sent} | ${m.received} | ${m.missing.length} | ${cell(m.latency_ms.p50)} | ${cell(m.latency_ms.p95)} | ${cell(m.latency_ms.max)} |`)
  }
  if (r.alerts.sent > 0) lines.push('', `Alerts drained in ${cell(r.alerts.drain_ms)} ms (presentation queue: show time plus gap per alert).`)
  const lost = [...r.chat.missing, ...r.alerts.missing]
  if (lost.length > 0) lines.push('', `**Missing:** ${lost.slice(0, 20).join(', ')}${lost.length > 20 ? ' …' : ''}`)
  lines.push('', '## Redemptions', '', `${r.redemptions.sent} sent, ${r.redemptions.recorded} recorded${r.redemptions.sent !== r.redemptions.recorded ? ' **(mismatch)**' : ''}.`)
  const res = r.resources
  lines.push('', '## App resources', '', '| | peak | average |', '|---|---|---|')
  lines.push(`| CPU % | ${cell(res.app_cpu_percent.peak)} | ${cell(res.app_cpu_percent.avg)} |`)
  lines.push(`| memory MB | ${cell(res.app_memory_mb.peak)} | ${cell(res.app_memory_mb.avg)} |`, '', `${res.samples} samples, every 2 s.`)
  const api = r.api ?? {}
  lines.push('', '## API calls', '', '| key | total | per minute | peak 10 s | min gap ms |', '|---|---|---|---|---|')
  for (const c of [...(api.calls ?? [])].sort((a, b) => b.total - a.total)) {
    lines.push(`| ${c.key} | ${c.total} | ${c.per_minute} | ${c.peak_10s} | ${cell(c.min_gap_ms)} |`)
  }
  lines.push('', `Helix: ${cell(api.helix?.total)} calls, peak ${cell(api.helix?.peak_per_minute)} a minute (${cell(api.helix?.bucket_percent)}% of 800).`)
  lines.push('', '## Warnings', '')
  const warnings = api.warnings ?? []
  if (warnings.length === 0) lines.push('None.')
  for (const w of warnings) lines.push(`- ${w.at_s} s, ${w.kind}, \`${w.key}\`: ${w.detail}`)
  return lines.join('\n') + '\n'
}

function args(argv) {
  const [scenario, ...rest] = argv
  const opts = { scenario, token: process.env.E2E_ADMIN_TOKEN, out: join(root, 'target', 'bench') }
  for (let i = 0; i < rest.length; i += 2) {
    const key = rest[i]?.replace(/^--/, '')
    if (!['host', 'fake', 'token', 'duration', 'out'].includes(key) || rest[i + 1] === undefined) {
      throw new Error(`unexpected argument ${rest[i]}`)
    }
    opts[key] = rest[i + 1]
  }
  if (!SCENARIOS[scenario]) throw new Error(`unknown scenario ${scenario}; one of ${Object.keys(SCENARIOS).join(', ')}`)
  if (!opts.host || !opts.fake || !opts.token) throw new Error('--host, --fake and E2E_ADMIN_TOKEN (or --token) are needed')
  opts.duration = Number(opts.duration ?? SCENARIOS[scenario].duration)
  return opts
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function run(o) {
  const control = async (path, body) => {
    const r = await fetch(`http://${o.fake}/_control/${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!r.ok) throw new Error(`${path}: ${r.status} ${await r.text()}`)
    return r.json()
  }
  const admin = async (path) => {
    const r = await fetch(`http://${o.host}${path}`, { headers: { Authorization: `Bearer ${o.token}` } })
    if (!r.ok) throw new Error(`${path}: ${r.status}`)
    return r.json()
  }
  const recorded = async () => (await admin('/api/admin/redemptions')).total ?? 0

  const chatSent = new Map()
  const chatGot = new Map()
  const alertSent = new Map()
  const alertGot = new Map()
  // Overlay sockets: the alerts one makes the presentation queue play; every socket gets every event.
  const open = (kind) =>
    new Promise((ok, fail) => {
      const ws = new WebSocket(`ws://${o.host}/ws/overlay?kind=${kind}`, { headers: { Origin: `http://${o.host}` } })
      ws.onopen = () => ok(ws)
      ws.onerror = () => fail(new Error(`cannot open the ${kind} overlay socket`))
    })
  const alerts = await open('alerts')
  const chatSocket = await open('chat')
  alerts.onmessage = (m) => {
    const now = performance.now()
    const e = JSON.parse(m.data)
    // `chat.message` carries no Twitch message ID; each bench message has its own text.
    if (e.type === 'chat.message' && e.payload?.text && !chatGot.has(e.payload.text)) chatGot.set(e.payload.text, now)
    if (e.type === 'alert' && e.payload?.userLogin && !alertGot.has(e.payload.userLogin)) alertGot.set(e.payload.userLogin, now)
  }

  const before = await recorded()
  await control('stats', { reset: true })
  const samples = []
  let polling = true
  const poller = (async () => {
    while (polling) {
      samples.push(await admin('/api/admin/performance').catch(() => ({})))
      await sleep(2000)
    }
  })()

  const started = new Date().toISOString()
  const t0 = performance.now()
  const end = t0 + o.duration * 1000
  const pending = []
  let redemptionsSent = 0
  const sendChat = (n) => {
    const text = `bench ${n}`
    chatSent.set(text, performance.now())
    pending.push(control('send', { kind: 'chat', user: `bench_chatter_${n % 200}`, message: text }))
  }
  const sendRedemption = (n) => {
    redemptionsSent++
    pending.push(control('send', { kind: 'redeem', user: `bench_viewer_${n}` }))
  }
  const every = async (ms, count, fn) => {
    for (let n = 0; (count === undefined || n < count) && performance.now() < end; n++) {
      fn(n)
      await sleep(ms)
    }
  }
  const s = o.scenario
  const loads = []
  if (s === 'chat' || s === 'all') loads.push(every(20, undefined, sendChat))
  if (s === 'redemptions' || s === 'all') {
    // The redemption scenario sends exactly 200 however long that takes; `all` keeps sending for the duration.
    loads.push(s === 'redemptions' ? (async () => { for (let n = 0; n < 200; n++) { sendRedemption(n); await sleep(50) } })() : every(50, undefined, sendRedemption))
  }
  if (s === 'obs' || s === 'all') {
    await control('obs', { action: 'meters', inputs: 30, interval: 50 })
    loads.push(
      (async () => {
        await control('obs', { action: 'burst', kind: 'scenes', count: 20 })
        await control('obs', { action: 'burst', kind: 'items', count: 20 })
        while (performance.now() < end) await sleep(100)
      })(),
    )
  }
  if (s === 'gifts') {
    const at = performance.now()
    pending.push(control('send', { kind: 'gift', user: 'bench_gifter', count: 100 }).then(() => alertSent.set('bench_gifter', at)))
  }
  if (s === 'idle' || s === 'gifts') loads.push(sleep(o.duration * 1000))
  await Promise.all(loads)
  await Promise.all(pending)
  if (s === 'obs' || s === 'all') await control('obs', { action: 'meters', inputs: 0, interval: 50 })
  const loadEnd = performance.now()

  // Drained: everything sent arrived (or was recorded), or 30 s passed.
  const drained = async () =>
    [...chatSent.keys()].every((id) => chatGot.has(id)) &&
    [...alertSent.keys()].every((id) => alertGot.has(id)) &&
    (await recorded()) - before >= redemptionsSent
  const drainLimit = performance.now() + 30_000
  while (performance.now() < drainLimit && !(await drained())) await sleep(200)
  const drainEnd = performance.now()
  polling = false
  await poller
  const api = await control('stats', {})
  alerts.close()
  chatSocket.close()

  const alertMatch = match(alertSent, alertGot)
  const lastAlert = Math.max(0, ...[...alertGot.values()])
  return {
    scenario: s,
    started_at: started,
    duration_s: Math.round((loadEnd - t0) / 100) / 10,
    drain_s: Math.round((drainEnd - loadEnd) / 100) / 10,
    chat: match(chatSent, chatGot),
    alerts: { ...alertMatch, drain_ms: alertSent.size > 0 && lastAlert > 0 ? Math.round(lastAlert - t0) : null },
    redemptions: { sent: redemptionsSent, recorded: (await recorded()) - before },
    resources: resources(samples),
    api,
  }
}

async function main() {
  let o
  try {
    o = args(process.argv.slice(2))
  } catch (e) {
    console.error(`${e.message}\nusage: node scripts/bench.mjs <${Object.keys(SCENARIOS).join('|')}> --host 127.0.0.1:<port> --fake 127.0.0.1:<port> [--duration <s>] [--token <t>] [--out <dir>]`)
    process.exit(2)
  }
  const report = await run(o)
  mkdirSync(o.out, { recursive: true })
  const name = `bench-${o.scenario}-${report.started_at.replace(/[:.]/g, '-')}`
  writeFileSync(join(o.out, `${name}.json`), JSON.stringify(report, null, 2))
  writeFileSync(join(o.out, `${name}.md`), markdown(report))
  console.log(markdown(report))
  console.log(`written: ${join(o.out, name)}.md and .json`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(e.message)
    process.exit(1)
  })
}
