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
  mock: 'profile.mock',
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
