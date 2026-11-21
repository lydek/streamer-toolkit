// `/overlay/sound/`: plays channel point reward sounds one at a time, in order. It shows nothing.
import { type Incoming, stopsStage } from '../chat/chat'
import { stopSounds } from '../sound'
import { playToEnd, soundQueue } from './queue'

const RETRY_MS = 2000
const enqueue = soundQueue(playToEnd)
let lastSeq = 0

function connect() {
  const ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=sound`)
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data) as Incoming
    // Sounds are never in snapshots, so none replays after a reconnect.
    if (msg.type === 'snapshot') lastSeq = msg.seq
    else if (msg.type === 'sound.play' && msg.seq > lastSeq) {
      lastSeq = msg.seq
      // The test button sends `source=simulation`; reward sounds are `twitch`.
      enqueue(msg.payload, msg.source === 'simulation')
    } else if (msg.type === 'sound.stopped' && msg.seq > lastSeq) {
      // The test button's stop: only test sounds go silent, reward sounds play on. One plays at a time, so stopping
      // every sound on this page stops just that one.
      lastSeq = msg.seq
      if (enqueue.clearTests()) stopSounds()
    } else if (stopsStage(msg, 'sound') && msg.seq > lastSeq) {
      // Emergency stop or skip: nothing waiting plays, and the sound playing goes silent.
      lastSeq = msg.seq
      enqueue.clear()
      stopSounds()
    }
  }
  ws.onclose = () => setTimeout(connect, RETRY_MS)
}

connect()
