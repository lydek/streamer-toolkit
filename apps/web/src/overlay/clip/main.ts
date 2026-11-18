// `/overlay/clip/`: shows the current SO clip through Twitch's official embed. Read-only: the Host decides what
// plays and when; this page only follows `clip.state`, from the snapshot after every (re)connect.
import { i18n } from '../../i18n'
import type { Incoming } from '../chat/chat'
import { type ClipState, render, view } from './clip'

const RETRY_MS = 2000
const root = document.getElementById('clip')!
const notice = i18n.global.t('overlay.clip.tooSmall')
let state: ClipState | null = null
let lastSeq = 0

const show = () => render(root, view(state, { width: innerWidth, height: innerHeight }, location.hostname), notice)

function connect() {
  const ws = new WebSocket(`ws://${location.host}/ws/overlay`)
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data) as Incoming
    if (msg.type === 'snapshot') {
      lastSeq = msg.seq
      state = msg.payload['clip.state'][0]?.payload ?? null
    } else if (msg.type === 'clip.state' && msg.seq > lastSeq) {
      lastSeq = msg.seq
      state = msg.payload
    } else return
    show()
  }
  ws.onclose = () => setTimeout(connect, RETRY_MS)
}

addEventListener('resize', show)
connect()
