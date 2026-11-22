import { i18n } from './i18n'

// Tests build expected copy with t(); a missing key would make both sides the bare key, so fail instead.
i18n.global.setMissingHandler((_locale, key) => {
  throw new Error(`missing locale key: ${key}`)
})
