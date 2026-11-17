import { beforeEach, describe, expect, it } from 'vitest'
import { DEFAULT_OVERRIDE_DURATION_SECS, usePreferencesStore } from './preferences'

describe('preferences store override duration', () => {
  beforeEach(() => {
    usePreferencesStore.setState({ overrideDurationSecs: DEFAULT_OVERRIDE_DURATION_SECS })
  })

  it('keeps the last valid duration', () => {
    usePreferencesStore.getState().setOverrideDurationSecs(120)
    expect(usePreferencesStore.getState().overrideDurationSecs).toBe(120)

    usePreferencesStore.getState().setOverrideDurationSecs('45')
    expect(usePreferencesStore.getState().overrideDurationSecs).toBe(45)
  })

  it('ignores values outside 1–3600 seconds', () => {
    usePreferencesStore.getState().setOverrideDurationSecs(120)
    for (const invalid of [0, -5, 3601, 'abc', null, undefined]) {
      usePreferencesStore.getState().setOverrideDurationSecs(invalid)
      expect(usePreferencesStore.getState().overrideDurationSecs).toBe(120)
    }
  })
})
