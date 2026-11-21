// `/overlay/bgm/`: plays the background music where the Host's player says it is, and 模擬事件 tests on the side.
// It shows nothing.
import type { Incoming } from '../chat/chat'
import { follow, testPlayer } from './player'

const RETRY_MS = 2000
const audio = new Audio()
const test = testPlayer(new Audio())
let lastSeq = 0

function connect() {
  const ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=bgm`)
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data) as Incoming
    if (msg.type === 'snapshot') {
      // A reload or reconnect picks up from the Host's position.
      lastSeq = msg.seq
      const last = msg.payload['bgm.player'].at(-1)
      if (last) follow(audio, last.payload, last.occurredAt)
    } else if (msg.type === 'bgm.player' && msg.seq > lastSeq) {
      lastSeq = msg.seq
      test.player(msg.payload)
      follow(audio, msg.payload, msg.occurredAt)
    } else if (msg.type === 'bgm.test') {
      test.play(msg.payload)
    }
  }
  ws.onclose = () => {
    // The Host's clock stops without an open overlay; so does the music.
    audio.pause()
    test.stop()
    setTimeout(connect, RETRY_MS)
  }
}

connect()
