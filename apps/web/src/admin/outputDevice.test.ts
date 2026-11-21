// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { chooseOutput, listOutputs, routeToOutput } from './outputDevice'

afterEach(() => {
  chooseOutput(undefined)
  vi.unstubAllGlobals()
})

const audio = (setSinkId: (id: string) => Promise<void>) => ({ setSinkId: vi.fn(setSinkId) }) as unknown as HTMLAudioElement

describe('routeToOutput', () => {
  it('plays on the chosen device, and on the default one when it is gone', async () => {
    chooseOutput({ id: 'headphones', label: '耳機' })
    const ok = audio(async () => {})
    await routeToOutput(ok)
    expect(ok.setSinkId).toHaveBeenCalledWith('headphones')

    const gone = audio(async (id) => {
      if (id) throw new DOMException('NotFoundError')
    })
    await routeToOutput(gone)
    expect(gone.setSinkId).toHaveBeenLastCalledWith('')
  })

  it('leaves the default alone when nothing is chosen', async () => {
    const a = audio(async () => {})
    await routeToOutput(a)
    expect(a.setSinkId).not.toHaveBeenCalled()
  })
})

describe('listOutputs', () => {
  it('asks for names only when told to, and releases the microphone at once', async () => {
    const stop = vi.fn()
    const getUserMedia = vi.fn(async () => ({ getTracks: () => [{ stop }] }))
    const enumerateDevices = vi.fn(async () => [
      { kind: 'audiooutput', deviceId: 'default', label: '' },
      { kind: 'audiooutput', deviceId: 'h1', label: '' },
      { kind: 'audioinput', deviceId: 'm1', label: '' },
    ])
    vi.stubGlobal('navigator', { mediaDevices: { getUserMedia, enumerateDevices } })
    expect(await listOutputs()).toEqual({ devices: [{ id: 'h1', label: '#1' }], named: false })
    expect(getUserMedia).not.toHaveBeenCalled()
    await listOutputs(true)
    expect(getUserMedia).toHaveBeenCalledTimes(1)
    expect(stop).toHaveBeenCalled()
  })

  it('still offers the names when the browser lists no output before the grant', async () => {
    const devices = [{ kind: 'audiooutput', deviceId: '', label: '' }]
    vi.stubGlobal('navigator', { mediaDevices: { enumerateDevices: async () => devices } })
    expect(await listOutputs()).toEqual({ devices: [], named: false })
    devices.splice(0, 1, { kind: 'audiooutput', deviceId: 'h1', label: 'Headset' })
    expect(await listOutputs()).toEqual({ devices: [{ id: 'h1', label: 'Headset' }], named: true })
  })
})
