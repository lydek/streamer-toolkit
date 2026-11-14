// Security check (docs/dev.md 安全測試): `invoke` from the template preview iframe must be rejected.
// Needs the real app, so it is not part of CI. Start it with WebView2's DevTools port and open the chat template
// editor (the page with the preview), then run:
//   set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
//   node scripts/check-preview-ipc.mjs 9333
// Exit code 0: rejected in the preview, allowed in the admin window (control). 1: anything else.
const port = process.argv[2] ?? '9333'
const PREVIEW = '/overlay/chat/?preview'
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
const admin = targets.find((t) => t.type === 'page' && t.url.includes('/admin'))
if (!admin) {
  console.error('No admin window. Targets:', targets.map((t) => `${t.type} ${t.url}`))
  process.exit(1)
}

/** One DevTools session; `contexts` collects the execution contexts of the target's in-process frames. */
async function session(target) {
  const ws = new WebSocket(target.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = reject
  })
  let id = 0
  const pending = new Map()
  const contexts = []
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data)
    if (m.id) pending.get(m.id)?.(m.result)
    else if (m.method === 'Runtime.executionContextCreated') contexts.push(m.params.context)
  }
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      pending.set(++id, resolve)
      ws.send(JSON.stringify({ id, method, params }))
    })
  await send('Runtime.enable')
  const evaluate = async (expression, contextId) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, contextId })
    return r.exceptionDetails ? `threw: ${r.exceptionDetails.exception?.description}` : r.result.value
  }
  return { ws, send, contexts, evaluate }
}

const main = await session(admin)
// The sandboxed (null-origin) preview may run out of process (its own target) or in the admin page's process.
const own = targets.find((t) => t.type === 'iframe' && t.url.includes(PREVIEW))
let inPreview
if (own) {
  const s = await session(own)
  inPreview = (expr) => s.evaluate(expr)
} else {
  const { frameTree } = await main.send('Page.getFrameTree')
  const frame = frameTree.childFrames?.find((c) => c.frame.url.includes(PREVIEW))?.frame
  const ctx = frame && main.contexts.find((c) => c.auxData?.frameId === frame.id && c.auxData?.isDefault)
  if (!ctx) {
    console.error('No preview iframe: open the chat template editor first.')
    process.exit(1)
  }
  inPreview = (expr) => main.evaluate(expr, ctx.id)
}

// Times out instead of hanging if Tauri never answers.
const call = `Promise.race([
  Promise.resolve().then(() => window.__TAURI_INTERNALS__.invoke('get_admin_token'))
    .then((t) => 'resolved ' + (typeof t === 'string' && t.length > 0), (e) => 'rejected ' + e),
  new Promise((r) => setTimeout(() => r('timeout'), 3000)),
])`
const origin = await inPreview('self.origin')
const fromPreview = await inPreview(call)
const fromAdmin = await main.evaluate(call)
console.log({ origin, fromPreview, fromAdmin })
const ok = origin === 'null' && !String(fromPreview).startsWith('resolved') && fromAdmin === 'resolved true'
console.log(ok ? 'PASS: invoke is rejected in the preview' : 'FAIL')
process.exit(ok ? 0 : 1)
