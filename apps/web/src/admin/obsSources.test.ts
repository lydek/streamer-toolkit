import { describe, expect, it } from 'vitest'
import { overlayRows } from './obsSources'

const source = (name: string, overlay: string, scenes: string[]) => ({ name, overlay, scenes })

describe('overlayRows', () => {
  it('gathers the sources of each overlay, the scenes they are in, and keeps grey rows for unbuilt ones', () => {
    const rows = overlayRows([
      source('Chat', '/overlay/chat/', ['Game', 'Talk']),
      source('Chat 2', '/overlay/chat/?x=1', ['Talk']),
      source('Cards', '/overlay/checkin/', ['Game']),
    ])
    const chat = rows.find((r) => r.key === 'chat')!
    expect(chat.sources.map((s) => s.name)).toEqual(['Chat', 'Chat 2'])
    expect(chat.scenes).toEqual(['Game', 'Talk'])
    expect(rows.find((r) => r.key === 'checkin')!.sources).toHaveLength(1)
    expect(rows.find((r) => r.key === 'bgm')!.sources).toEqual([])
    const songs = rows.find((r) => r.key === 'songs')!
    expect([songs.url, songs.entry.path]).toEqual([undefined, undefined])
  })

  it('gives a leaderboard one row per board and period in OBS, then 「其他」', () => {
    const rows = overlayRows([
      source('Bits', '/overlay/leaderboard/?board=bits&period=stream', ['Game']),
      source('Bits again', '/overlay/leaderboard/?board=bits&period=stream', ['Talk']),
      source('Broken', '/overlay/leaderboard/', []),
    ]).filter((r) => r.entry.id === 'leaderboard')
    expect(rows.map((r) => [r.key, r.board, r.sources.length])).toEqual([
      ['/overlay/leaderboard/?board=bits&period=stream', { board: 'bits', period: 'stream' }, 2],
      ['leaderboard-other', undefined, 0],
    ])
  })

  it('leaves out the overlays of modules turned off', () => {
    const ids = overlayRows([], new Set(['counter'])).map((r) => r.key)
    expect(ids).not.toContain('counter')
    expect(ids).not.toContain('marathon')
    expect(ids).toContain('chat')
  })
})
