// `/overlay/sound/`: plays channel point reward sounds one at a time, in order. It shows nothing.
import type { Incoming } from '../chat/chat'
import { playToEnd, soundQueue } from './queue'

const RETRY_MS = 2000
const enqueue = soundQueue(playToEnd)
let lastSeq = 0

function connect() {
  const ws = new WebSocket(`ws://${location.host}/ws/overlay`)
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data) as Incoming
    // Sounds are never in snapshots, so none replays after a reconnect.
    if (msg.type === 'snapshot') lastSeq = msg.seq
    else if (msg.type === 'sound.play' && msg.seq > lastSeq) {
      lastSeq = msg.seq
      enqueue(msg.payload)
    }
  }
  ws.onclose = () => setTimeout(connect, RETRY_MS)
}

connect()
