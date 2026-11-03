import { useEffect } from 'react'
import { useBeaconStore } from '../store/beacon'

export function useBeaconStream() {
  const setSnapshot = useBeaconStore((state) => state.setSnapshot)
  const setConnected = useBeaconStore((state) => state.setConnected)

  useEffect(() => {
    let source = null
    let timer = null
    let stopped = false
    let retry = 800

    const connect = () => {
      source = new EventSource('/api/v1/stream')
      source.onopen = () => {
        retry = 800
        setConnected(true)
      }
      source.addEventListener('snapshot', (event) => {
        setSnapshot(JSON.parse(event.data))
        setConnected(true)
      })
      source.addEventListener('update', (event) => {
        setSnapshot(JSON.parse(event.data))
      })
      source.onerror = () => {
        setConnected(false)
        source?.close()
        if (!stopped) {
          timer = setTimeout(connect, retry)
          retry = Math.min(retry * 2, 8000)
        }
      }
    }

    connect()
    return () => {
      stopped = true
      clearTimeout(timer)
      source?.close()
    }
  }, [setSnapshot, setConnected])
}
