import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { MAX_BACKOFF_MS, useBeaconPolling } from './useBeaconPolling'
import { useBeaconStore } from '../store/beacon'

const snapshot = (overrides) => ({
  aggregate: 'idle',
  effect: 'calm',
  sessions: [],
  devices: [],
  routing: [],
  ...overrides,
})

const jsonResponse = (body, etag = '"1"') => ({
  ok: true,
  status: 200,
  headers: { get: (name) => (name === 'ETag' ? etag : null) },
  json: async () => body,
})

describe('useBeaconPolling', () => {
  beforeEach(() => {
    useBeaconStore.setState({ snapshot: null, connected: false, loading: true, error: null })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it('polls with the previous ETag and applies 200 responses', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(snapshot({ aggregate: 'working' }), '"7"'))
      .mockResolvedValue({ ok: false, status: 304, headers: { get: () => null } })
    vi.stubGlobal('fetch', fetchMock)

    renderHook(() => useBeaconPolling())

    await waitFor(() => {
      expect(useBeaconStore.getState().snapshot?.aggregate).toBe('working')
    })
    expect(useBeaconStore.getState().connected).toBe(true)
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/state', { headers: {} })

    // 第二次带上上次的 ETag；304 时不动快照，只把连接状态标成在线。
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(fetchMock.mock.calls[1]).toEqual([
      '/api/v1/state',
      { headers: { 'If-None-Match': '"7"' } },
    ])
    expect(useBeaconStore.getState().snapshot?.aggregate).toBe('working')

    // 轮询不会像 SSE 那样持有连接：每次都是独立的 GET。
    expect(useBeaconStore.getState().connected).toBe(true)
  })

  it('backs off and reports offline when the request fails', async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn().mockRejectedValue(new Error('boom'))
    vi.stubGlobal('fetch', fetchMock)

    renderHook(() => useBeaconPolling())

    // 首次请求立刻失败：标记离线，并按 1 秒退避安排重试。
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(useBeaconStore.getState().connected).toBe(false)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(999)
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1)
    })
    expect(fetchMock).toHaveBeenCalledTimes(2)

    // 第二次失败后退避翻倍（2 秒），1.5 秒内不会重试。
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_500)
    })
    expect(fetchMock).toHaveBeenCalledTimes(2)
    await act(async () => {
      await vi.advanceTimersByTimeAsync(500)
    })
    expect(fetchMock).toHaveBeenCalledTimes(3)
    expect(MAX_BACKOFF_MS).toBeGreaterThanOrEqual(1_000)
  })
})
