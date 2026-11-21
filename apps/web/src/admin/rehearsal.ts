import { ApiError, type Schemas, api, client } from './api'

export type Speed = Schemas['Speed']

/** The Host's limit (crates/host/src/rehearsal.rs); checked here first so a large file is never sent. */
export const SCRIPT_MAX_BYTES = 256 * 1024

type Step =
  | { type: 'chat'; at: number; userName: string; text: string }
  | { type: 'alert'; at: number; kind: string; userName?: string; amount?: number; message?: string }
  | { type: 'checkin'; at: number; userName: string; stamps: number }
  | { type: 'stop'; at: number }

const names = ['小魚', '莓莓', '阿凱', '橘子', 'Kai', '小熊', '毛毛', '阿哲', '小米', '泡芙', '可可', '布丁']
const seconds = (s: number) => s * 1000

export const BUILT_IN = ['checkins', 'raid', 'opening', 'stop'] as const
export type BuiltIn = (typeof BUILT_IN)[number]

/**
 * A built-in script, in the same JSON a broadcaster's own file uses. Stamps fit a card of `slots` (the Host refuses
 * more), and each script that shows cards ends on a full one.
 */
export const builtIn = (id: BuiltIn, slots: number) => scripts(slots)[id]

const scripts = (slots: number): Record<BuiltIn, { steps: Step[] }> => ({
  checkins: {
    steps: names.slice(0, 10).map((userName, i) => ({
      type: 'checkin',
      at: seconds(i),
      userName,
      stamps: i === 9 ? slots : Math.min(i + 1, slots),
    })),
  },
  raid: {
    steps: [
      { type: 'alert', at: 0, kind: 'raid', userName: '大部隊', amount: 120 },
      ...names.map((userName, i): Step => ({ type: 'chat', at: seconds(1 + i * 0.7), userName, text: '揪團來囉！' })),
      { type: 'alert', at: seconds(6), kind: 'follow', userName: '新朋友' },
    ],
  },
  opening: {
    steps: [
      ...names.slice(0, 8).map((userName, i): Step => ({ type: 'chat', at: seconds(i * 0.5), userName, text: '開台啦～' })),
      { type: 'alert', at: seconds(2), kind: 'follow', userName: '路人甲' },
      { type: 'alert', at: seconds(4), kind: 'subscribe', userName: '阿凱' },
      { type: 'alert', at: seconds(7), kind: 'cheer', userName: '橘子', amount: 100, message: '加油！' },
      { type: 'alert', at: seconds(10), kind: 'gift', userName: '莓莓', amount: 3 },
      { type: 'checkin', at: seconds(12), userName: '小魚', stamps: slots },
    ],
  },
  stop: {
    steps: [
      { type: 'alert', at: 0, kind: 'follow', userName: '一號' },
      { type: 'alert', at: 200, kind: 'subscribe', userName: '二號' },
      { type: 'alert', at: 400, kind: 'raid', userName: '三號', amount: 30 },
      { type: 'checkin', at: 600, userName: '四號', stamps: Math.min(5, slots) },
      { type: 'stop', at: seconds(3) },
    ],
  },
})

/** Plays a built-in script fitted to the current card; without the check-in settings, a card of 10. */
export async function runBuiltIn(id: BuiltIn, speed: Speed) {
  const slots = await api(client.GET('/api/admin/checkin/settings')).then(
    (s) => s.card_slots,
    () => 10,
  )
  return runScript(JSON.stringify(builtIn(id, slots)), speed)
}

/** Plays a script's text; the Host makes every step a simulation and refuses a broken file naming where. */
export function runScript(script: string, speed: Speed) {
  if (new Blob([script]).size > SCRIPT_MAX_BYTES) return Promise.reject(new ApiError('rehearsal.too_large'))
  return api(client.POST('/api/admin/simulate/script', { body: { script, speed } }))
}

export const stopScript = () => api(client.POST('/api/admin/simulate/script/stop'))
