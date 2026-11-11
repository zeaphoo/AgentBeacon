export const EFFECTS = [
  { value: 'off', labelKey: 'effect.off.label', descriptionKey: 'effect.off.description', tone: 'muted' },
  { value: 'calm', labelKey: 'effect.calm.label', descriptionKey: 'effect.calm.description', tone: 'green' },
  { value: 'busy', labelKey: 'effect.busy.label', descriptionKey: 'effect.busy.description', tone: 'yellow' },
  {
    value: 'attention',
    labelKey: 'effect.attention.label',
    descriptionKey: 'effect.attention.description',
    tone: 'amber',
  },
  { value: 'alert', labelKey: 'effect.alert.label', descriptionKey: 'effect.alert.description', tone: 'red' },
]

export const EFFECT_MAP = Object.fromEntries(EFFECTS.map((effect) => [effect.value, effect]))

export const AGGREGATE_KEYS = {
  none: 'aggregate.none',
  idle: 'aggregate.idle',
  working: 'aggregate.working',
  waiting: 'aggregate.waiting',
  error: 'aggregate.error',
}

export const STATE_KEYS = {
  idle: 'state.idle',
  working: 'state.working',
  waiting: 'state.waiting',
  error: 'state.error',
}

export const PROFILE_KEYS = {
  'mlight-single': 'profile.mlight-single',
  'mlight-3led': 'profile.mlight-3led',
  'yh50-rgy-j': 'profile.yh50-rgy-j',
  'agentcore-light': 'profile.agentcore-light',
}


export const PROFILE_CAPABILITIES = {
  'mlight-single': { brightness: true, buzzer: false },
  'mlight-3led': { brightness: true, buzzer: false },
  'yh50-rgy-j': { brightness: false, buzzer: true },
  'agentcore-light': { brightness: true, buzzer: true },
}

export const SUPPORTED_LIGHT_PROFILES = [
  'mlight-single',
  'mlight-3led',
  'yh50-rgy-j',
  'agentcore-light',
]

export function capabilitiesForProfile(profile, profiles = []) {
  const info = profiles?.find((item) => item.profile === profile)
  return info?.capabilities ?? PROFILE_CAPABILITIES[profile] ?? { brightness: false, buzzer: false }
}

export const TONE_CLASSES = {
  muted: 'tone-muted',
  green: 'tone-green',
  yellow: 'tone-yellow',
  amber: 'tone-amber',
  red: 'tone-red',
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

export function stateTone(state, mapping = {}) {
  const effect = mapping?.[state]
  return effect ? effectTone(effect) : (STATE_TONE[state] ?? 'muted')
}

export function effectLabel(effect, t) {
  const key = EFFECT_MAP[effect]?.labelKey
  return key ? t(key) : effect
}

export function effectDescription(effect, t) {
  const key = EFFECT_MAP[effect]?.descriptionKey
  return key ? t(key) : ''
}

export function aggregateLabel(aggregate, t) {
  const key = AGGREGATE_KEYS[aggregate]
  return key ? t(key) : aggregate
}

export function stateLabel(state, t) {
  const key = STATE_KEYS[state]
  return key ? t(key) : state
}

export function profileLabel(profile, t) {
  const key = PROFILE_KEYS[profile]
  return key ? t(key) : profile
}


const CONNECTION_KEYS = {
  bluetooth: 'connection.bluetooth',
  bluetoothSpp: 'connection.bluetoothSpp',
  serial: 'connection.serial',
  usb: 'connection.usb',
  device: 'connection.device',
}

export function connectionKind(port) {
  const value = String(port ?? '').trim().toLowerCase()
  if (value.startsWith('ble:')) return 'bluetooth'
  if (value.startsWith('spp:')) return 'bluetoothSpp'
  if (value.startsWith('usb:')) return 'usb'
  if (
    value.startsWith('serial:') ||
    value.startsWith('port:') ||
    value.startsWith('/dev/') ||
    value.startsWith('rfcomm') ||
    /^com\d+$/.test(value) ||
    value.includes('bluetooth')
  ) {
    return 'serial'
  }
  return 'device'
}

export function connectionLabel(port, t) {
  const kind = connectionKind(port)
  return t(CONNECTION_KEYS[kind])
}

export function formatAge(timestampMs, language = 'zh', now = Date.now()) {
  if (!timestampMs) return '-'
  const seconds = Math.max(0, Math.round((now - timestampMs) / 1000))
  const formatter = new Intl.RelativeTimeFormat(language === 'zh' ? 'zh-CN' : 'en', {
    numeric: 'auto',
  })
  if (seconds < 60) return formatter.format(-seconds, 'second')
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return formatter.format(-minutes, 'minute')
  const hours = Math.round(minutes / 60)
  return formatter.format(-hours, 'hour')
}

export function formatDateTime(timestampMs, language = 'zh') {
  if (!timestampMs) return '-'
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(timestampMs))
}

export function formatCountdown(timestampMs, language = 'zh', now = Date.now()) {
  if (!timestampMs) return '-'
  const seconds = Math.round((timestampMs - now) / 1000)
  const formatter = new Intl.RelativeTimeFormat(language === 'zh' ? 'zh-CN' : 'en', {
    numeric: 'auto',
  })
  if (Math.abs(seconds) < 60) return formatter.format(seconds, 'second')
  const minutes = Math.round(seconds / 60)
  if (Math.abs(minutes) < 60) return formatter.format(minutes, 'minute')
  return formatter.format(Math.round(minutes / 60), 'hour')
}

export function formatUptime(seconds, t) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0))
  if (total < 60) return t('time.seconds', { n: total })

  const minutes = Math.floor(total / 60)
  if (minutes < 60) return t('time.minutes', { n: minutes })

  const hours = Math.floor(minutes / 60)
  const restMinutes = minutes % 60
  if (hours < 24) {
    return restMinutes > 0
      ? `${t('time.hours', { n: hours })} ${t('time.minutes', { n: restMinutes })}`
      : t('time.hours', { n: hours })
  }

  const days = Math.floor(hours / 24)
  const restHours = hours % 24
  return restHours > 0
    ? `${t('time.days', { n: days })} ${t('time.hours', { n: restHours })}`
    : t('time.days', { n: days })
}
