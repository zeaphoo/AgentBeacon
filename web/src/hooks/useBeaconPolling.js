import { useEffect } from 'react'
import { api } from '../lib/api'
import { useBeaconStore } from '../store/beacon'

/// 前台轮询间隔：数据源本身是 5 秒心跳，1 秒足够跟手。
export const VISIBLE_INTERVAL_MS = 1_000
/// 标签页在后台时降频，别为看不见的界面发请求。
export const HIDDEN_INTERVAL_MS = 5_000
/// 出错后的最小/最大退避。
export const MIN_BACKOFF_MS = 1_000
export const MAX_BACKOFF_MS = 8_000

/// 轮询 `/api/v1/state`，用 ETag 做条件请求（没变就是 304，不传快照）。
///
/// 相比 SSE：不占浏览器同域连接槽位、不需要重连/心跳逻辑，断线也天然自愈。
export function useBeaconPolling() {
  const setSnapshot = useBeaconStore((state) => state.setSnapshot)
  const setConnected = useBeaconStore((state) => state.setConnected)

  useEffect(() => {
    let stopped = false
    let timer = null
    let etag = null
    let backoff = MIN_BACKOFF_MS

    const schedule = (delay) => {
      clearTimeout(timer)
      if (stopped) return
      timer = setTimeout(poll, delay)
    }

    const nextDelay = () =>
      typeof document !== 'undefined' && document.hidden
        ? HIDDEN_INTERVAL_MS
        : VISIBLE_INTERVAL_MS

    async function poll() {
      if (stopped) return
      try {
        const result = await api.getStateIfChanged(etag)
        if (stopped) return
        if (!result.notModified) {
          etag = result.etag
          setSnapshot(result.snapshot)
        }
        setConnected(true)
        backoff = MIN_BACKOFF_MS
        schedule(nextDelay())
      } catch {
        if (stopped) return
        setConnected(false)
        schedule(backoff)
        backoff = Math.min(backoff * 2, MAX_BACKOFF_MS)
      }
    }

    // 从后台切回前台时立刻拉一次，不用等下一个周期。
    const onVisibility = () => {
      if (typeof document === 'undefined' || document.hidden) return
      clearTimeout(timer)
      poll()
    }
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', onVisibility)
    }
    poll()

    return () => {
      stopped = true
      clearTimeout(timer)
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisibility)
      }
    }
  }, [setSnapshot, setConnected])
}
