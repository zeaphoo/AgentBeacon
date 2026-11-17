import { useEffect, useState } from 'react'
import { Usb } from 'lucide-react'
import agentcoreLight from '../assets/devices/agentcore-light.png'
import agentcoreHalo from '../assets/devices/agentcore-halo.png'
import { cn } from '../lib/utils'

/**
 * 设备型号配图。
 *
 * 素材不是方形：固定槽位 + `object-contain` 保持原比例，再用每型号的 `scale`
 * 补偿视觉大小。`transform` 不参与布局，所以行高、卡片高度都不会因为缩放变化。
 *
 * - `mlight-3led` 与 `agentcore-halo` 外观相同，共用同一张图。
 * - 未收录的型号或图片加载失败时回落到通用 USB 图标，不出现空位或裂图。
 * - 断开时灰度 + 半透明；型号名始终由旁边的文字给出，图片对读屏隐藏。
 */
export const DEVICE_IMAGES = {
  'agentcore-light': { src: agentcoreLight, scale: 1.12 },
  'agentcore-halo': { src: agentcoreHalo, scale: 0.92 },
  'mlight-3led': { src: agentcoreHalo, scale: 0.92 },
}

export function deviceImageScale(profile) {
  return DEVICE_IMAGES[profile]?.scale ?? 1
}

export default function DeviceImage({ profile, connected = true, showImage = true, className }) {
  const entry = showImage ? DEVICE_IMAGES[profile] : null
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    setFailed(false)
  }, [profile])

  if (!entry || failed) {
    return (
      <span
        data-device-image="fallback"
        aria-hidden="true"
        className={cn(
          'flex shrink-0 items-center justify-center text-muted-foreground',
          !connected && 'opacity-50',
          className,
        )}
      >
        <Usb className="h-1/2 w-1/2" />
      </span>
    )
  }

  return (
    <span
      data-device-image={profile}
      aria-hidden="true"
      className={cn('flex shrink-0 items-center justify-center', className)}
    >
      <img
        src={entry.src}
        alt=""
        aria-hidden="true"
        draggable={false}
        onError={() => setFailed(true)}
        style={{ transform: `scale(${entry.scale})` }}
        className={cn(
          'h-full w-full object-contain',
          !connected && 'opacity-50 grayscale',
        )}
      />
    </span>
  )
}
