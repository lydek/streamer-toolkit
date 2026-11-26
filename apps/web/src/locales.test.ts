import { expect, it } from 'vitest'

// i18n.ts merges the files with Object.assign, so a top-level key in two files would silently drop one copy.
it('gives each top-level zh-TW key to exactly one file', () => {
  const files = import.meta.glob<Record<string, unknown>>('./locales/zh-TW/*.json', { eager: true, import: 'default' })
  const owners = new Map<string, string[]>()
  for (const [file, messages] of Object.entries(files)) {
    for (const key of Object.keys(messages)) owners.set(key, [...(owners.get(key) ?? []), file])
  }
  expect([...owners].filter(([, paths]) => paths.length > 1)).toEqual([])
  expect(owners.has('errors')).toBe(true)
})
