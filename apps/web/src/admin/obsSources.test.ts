import { describe, expect, it, vi } from 'vitest'
import { client } from './api'
import { addOverlay, obsPresence, obsProblems, overlayRows, sourceSize, useObsSources } from './obsSources'

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

describe('obsPresence', () => {
  const data = (sources: ReturnType<typeof source>[]) => ({ connected: true, local: true, live_scene: 'Game', scenes: ['Game', 'Talk'], sources })

  it('says whether OBS is offline, has the overlay nowhere, twice, or in how many scenes with the live one', () => {
    expect(obsPresence('/overlay/chat/', undefined)).toEqual({ kind: 'offline' })
    expect(obsPresence('/overlay/chat/', { ...data([]), connected: false })).toEqual({ kind: 'offline' })
    expect(obsPresence('/overlay/chat/', data([]))).toEqual({ kind: 'missing' })
    expect(obsPresence('/overlay/chat/', data([source('A', '/overlay/chat/', [])]))).toEqual({ kind: 'missing' })
    expect(obsPresence('/overlay/chat/', data([source('A', '/overlay/chat/', ['Talk'])]))).toEqual({ kind: 'in', scenes: 1, live: false })
    expect(obsPresence('/overlay/chat/', data([source('A', '/overlay/chat/?x', ['Game', 'Talk'])]))).toEqual({ kind: 'in', scenes: 2, live: true })
    expect(obsPresence('/overlay/chat/', data([source('A', '/overlay/chat/', []), source('B', '/overlay/chat/', [])]))).toEqual({
      kind: 'duplicate',
      sources: 2,
    })
  })

  it('matches a leaderboard by its board and period', () => {
    const lb = data([source('A', '/overlay/leaderboard/?board=bits&period=stream', ['Game'])])
    expect(obsPresence('/overlay/leaderboard/?board=bits&period=stream', lb).kind).toBe('in')
    expect(obsPresence('/overlay/leaderboard/?board=chat&period=stream', lb).kind).toBe('missing')
  })
})

describe('obsProblems', () => {
  const t = (key: string, values?: Record<string, unknown>) => `${key} ${JSON.stringify(values ?? {})}`
  const data = (sources: ReturnType<typeof source>[]) => ({ connected: true, local: true, live_scene: 'Talk', scenes: ['Game', 'Talk'], sources })
  const rule = (scenes: string[], enabled = true) => ({
    id: 'r', name: 'Talk music', autoplay: true, enabled, playlist: 'p', scenes, shuffle: false, stop_on_leave: false, volume: 100,
  })

  it('lists an overlay with two sources, and background music missing from a live scene with a playlist', () => {
    const problems = obsProblems(
      data([source('A', '/overlay/checkin/', []), source('B', '/overlay/checkin/', []), source('M', '/overlay/bgm/', ['Game'])]),
      [rule(['talk'])],
      new Set(),
      t,
    )
    expect(problems.map((p) => [p.id, p.to])).toEqual([
      ['duplicate:checkin', '/obs-sources'],
      ['bgm:scene', '/obs-sources'],
    ])
  })

  it('has nothing once the causes are gone, or while OBS is offline', () => {
    const fixed = data([source('A', '/overlay/checkin/', []), source('M', '/overlay/bgm/', ['Talk'])])
    expect(obsProblems(fixed, [rule(['Talk'])], new Set(), t)).toEqual([])
    expect(obsProblems(data([]), [rule(['Talk'], false)], new Set(), t)).toEqual([])
    expect(obsProblems(data([]), [rule(['Talk'])], new Set(['bgm']), t)).toEqual([])
    expect(obsProblems(undefined, [rule(['Talk'])], new Set(), t)).toEqual([])
  })
})

describe('addOverlay', () => {
  it('creates the source at the suggested size, named after the overlay', async () => {
    const post = vi.spyOn(client, 'POST').mockResolvedValue({ data: undefined, response: new Response(null, { status: 204 }) } as never)
    const t = (key: string, values?: Record<string, unknown>) => (key === 'obsSources.sourceName' ? `${values!.name}（${values!.app}）` : key)
    const chat = overlayRows([]).find((r) => r.key === 'chat')!
    await addOverlay('Game', chat, t)
    expect(post).toHaveBeenCalledWith('/api/admin/obs/sources', {
      body: { scene: 'Game', overlay: '/overlay/chat/', name: 'obsSources.names.chat（app.name）', width: 400, height: 600 },
    })
    expect(sourceSize('audio')).toEqual([100, 100])
    post.mockRestore()
  })
})

describe('useObsSources', () => {
  it('reads once more after a read asked for right now while another was under way', async () => {
    const answers: ((v: unknown) => void)[] = []
    const get = vi.spyOn(client, 'GET').mockImplementation((() => new Promise((resolve) => answers.push(resolve))) as never)
    const shared = useObsSources(true)
    useObsSources(true)
    expect(get).toHaveBeenCalledTimes(1)
    const answer = (scenes: string[]) =>
      answers.at(-1)!({ data: { connected: true, local: true, live_scene: null, scenes, sources: [] }, response: new Response(null, { status: 200 }) })
    answer(['Old'])
    await vi.waitFor(() => expect(get).toHaveBeenCalledTimes(2))
    answer(['Old', 'New'])
    await vi.waitFor(() => expect(shared.value?.scenes).toEqual(['Old', 'New']))
    get.mockRestore()
  })
})
