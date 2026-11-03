export const EFFECTS = [
  { value: 'off', label: '关闭', description: '全部熄灭', tone: 'muted' },
  { value: 'calm', label: '平静', description: '绿色常亮', tone: 'green' },
  { value: 'busy', label: '运行中', description: '绿常亮 + 黄呼吸', tone: 'yellow' },
  { value: 'attention', label: '需要关注', description: '黄色闪烁', tone: 'amber' },
  { value: 'alert', label: '告警', description: '红色闪烁', tone: 'red' },
]

export const EFFECT_MAP = Object.fromEntries(EFFECTS.map((effect) => [effect.value, effect]))

export const AGGREGATE_LABELS = {
  none: '无活动',
  idle: '空闲',
  working: '运行中',
  waiting: '等待确认',
  error: '错误',
}

export const STATE_LABELS = {
  idle: '空闲',
  working: '运行中',
  waiting: '等待确认',
  error: '错误',
}

export const PROFILE_LABELS = {
  'mlight-single': 'MLight 单灯珠',
  'mlight-3led': 'MLight 三灯珠',
  'yh50-rgy-j': 'YH50-RGY-J',
  mock: 'Mock 设备',
}

export const TONE_CLASSES = {
  muted: 'bg-muted text-muted-foreground border-border',
  green: 'bg-emerald-500/15 text-emerald-300 border-emerald-500/30',
  yellow: 'bg-yellow-500/15 text-yellow-200 border-yellow-500/30',
  amber: 'bg-amber-500/15 text-amber-200 border-amber-500/30',
  red: 'bg-red-500/15 text-red-300 border-red-500/30',
}

export const STATE_TONE = {
  idle: 'muted',
  working: 'green',
  waiting: 'amber',
  error: 'red',
}

export function effectTone(effect) {
  return EFFECT_MAP[effect]?.tone ?? 'muted'
}

export function formatAge(timestampMs, now = Date.now()) {
  if (!timestampMs) return '-'
  const seconds = Math.max(0, Math.round((now - timestampMs) / 1000))
  if (seconds < 60) return `${seconds} 秒前`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} 分钟前`
  const hours = Math.round(minutes / 60)
  return `${hours} 小时前`
}
