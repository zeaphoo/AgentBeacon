import { useCallback, useEffect, useMemo, useState } from 'react'
import { ChevronDown, RefreshCw, ScrollText, Search } from 'lucide-react'
import { api } from '../lib/api'
import { useI18n } from '../i18n'
import {
  TONE_CLASSES,
  aggregateLabel,
  effectLabel,
  formatAge,
  formatDateTime,
  stateLabel,
} from '../lib/labels'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { Input } from './ui/input'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from './ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select'
import { cn } from '../lib/utils'

/// 服务端内存里最多保留 2000 条记录（journal::DEFAULT_CAPACITY）。
const HOOK_LOG_LIMIT = 2000
const ALL_AGENTS = '__all__'

/// 原始上报能解析成 JSON 就美化一下，否则原样显示。
function prettyRaw(raw) {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2)
  } catch {
    return raw
  }
}

function HookRow({ entry, language, t, expanded, onToggle }) {
  const rejected = entry.outcome === 'rejected'
  return (
    <li className="border-b border-border/60 last:border-b-0">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={onToggle}
        className="flex w-full items-center gap-2.5 py-2 text-left"
      >
        <span className="w-[9.5rem] shrink-0 font-mono text-[11px] text-muted-foreground">
          {formatDateTime(entry.at_ms, language)}
        </span>
        <Badge
          variant="outline"
          className="min-w-[5rem] justify-center border border-border text-[11px] tone-muted"
        >
          {t(`hook.kind.${entry.kind}`)}
        </Badge>
        {entry.event ? (
          <span className="max-w-40 shrink-0 truncate rounded border border-border/60 px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground">
            {entry.event}
          </span>
        ) : null}
        <span className="min-w-0 flex-1 truncate text-xs">
          <span className="font-medium">{entry.agent ?? '-'}</span>
          {entry.session ? (
            <span className="text-muted-foreground"> / {entry.session}</span>
          ) : null}
          {entry.message ? (
            <span className="text-muted-foreground"> · {entry.message}</span>
          ) : null}
        </span>
        {entry.state ? (
          <span className="shrink-0 text-xs text-muted-foreground">
            {stateLabel(entry.state, t)}
          </span>
        ) : null}
        <Badge
          variant="outline"
          className={cn(
            'min-w-[4.5rem] shrink-0 justify-center border text-[11px]',
            rejected ? TONE_CLASSES.red : TONE_CLASSES.muted,
          )}
        >
          {t(rejected ? 'hook.outcome.rejected' : 'hook.outcome.applied')}
        </Badge>
        <span className="w-[4.5rem] shrink-0 text-right text-xs text-muted-foreground">
          {formatAge(entry.at_ms, language)}
        </span>
        <ChevronDown
          className={cn(
            'h-4 w-4 shrink-0 text-muted-foreground transition-transform',
            expanded && 'rotate-180',
          )}
        />
      </button>
      {expanded && (
        <dl className="mb-2 grid gap-1.5 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5 text-xs sm:grid-cols-[7rem_1fr]">
          <DetailRow label={t('hook.fieldSeq')} value={`#${entry.seq}`} />
          <DetailRow
            label={t('hook.fieldAggregate')}
            value={`${aggregateLabel(entry.aggregate_before, t)} → ${aggregateLabel(entry.aggregate_after, t)}`}
          />
          <DetailRow
            label={t('hook.fieldEffect')}
            value={`${effectLabel(entry.effect_before, t)} → ${effectLabel(entry.effect_after, t)}`}
          />
          {entry.event ? (
            <DetailRow label={t('hook.fieldEvent')} value={entry.event} />
          ) : null}
          {entry.ttl_secs ? (
            <DetailRow label={t('hook.fieldTtl')} value={`${entry.ttl_secs}s`} />
          ) : null}
          {entry.error ? (
            <DetailRow label={t('hook.fieldError')} value={entry.error} danger />
          ) : null}
          {entry.detail ? (
            <DetailRow label={t('hook.fieldDetail')} value={entry.detail} />
          ) : null}
          {entry.raw ? (
            <div className="contents">
              <dt className="text-muted-foreground">{t('hook.fieldRaw')}</dt>
              <dd>
                <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border/60 bg-background/70 p-2 font-mono text-[11px] leading-relaxed">
                  {prettyRaw(entry.raw)}
                </pre>
              </dd>
            </div>
          ) : null}
        </dl>
      )}
    </li>
  )
}

function DetailRow({ label, value, danger }) {
  return (
    <div className="contents">
      <dt className="text-muted-foreground">{label}</dt>
      <dd
        className={cn(
          'break-all font-mono text-[11px]',
          danger ? 'text-danger' : 'text-foreground',
        )}
      >
        {value}
      </dd>
    </div>
  )
}

/**
 * 事件 hook 日志弹窗：读取服务端内存里的最近记录（不落盘），
 * 支持按 agent 与关键词过滤，单行可展开查看状态迁移与详情。
 */
export default function HookLogDialog() {
  const { t, language } = useI18n()
  const [open, setOpen] = useState(false)
  const [data, setData] = useState(null)
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState(null)
  const [agent, setAgent] = useState(ALL_AGENTS)
  const [keyword, setKeyword] = useState('')
  const [expanded, setExpanded] = useState(null)

  const load = useCallback(async () => {
    setLoading(true)
    setLoadError(null)
    try {
      const result = await api.getHooks(HOOK_LOG_LIMIT)
      setData(result)
    } catch (err) {
      setLoadError(err.message)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (open) void load()
  }, [open, load])

  const entries = useMemo(() => data?.entries ?? [], [data])
  const agents = useMemo(
    () => [...new Set(entries.map((entry) => entry.agent).filter(Boolean))].sort(),
    [entries],
  )
  const filtered = useMemo(() => {
    const needle = keyword.trim().toLowerCase()
    return entries
      .filter((entry) => agent === ALL_AGENTS || entry.agent === agent)
      .filter((entry) => {
        if (!needle) return true
        return [
          entry.agent,
          entry.session,
          entry.message,
          entry.event,
          entry.raw,
          entry.detail,
          entry.error,
          entry.kind,
          entry.state,
          entry.outcome,
        ]
          .filter(Boolean)
          .some((value) => String(value).toLowerCase().includes(needle))
      })
      .reverse()
  }, [entries, agent, keyword])

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <ScrollText className="mr-1.5 h-3.5 w-3.5" />
          {t('settings.hooksOpen')}
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle>{t('settings.hooksTitle')}</DialogTitle>
          <DialogDescription>
            {t('settings.hooksDescription', {
              capacity: data?.capacity ?? HOOK_LOG_LIMIT,
              count: entries.length,
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-wrap items-center gap-2">
          <Select value={agent} onValueChange={setAgent}>
            <SelectTrigger className="h-9 w-40" aria-label={t('settings.hooksFilterAgent')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_AGENTS}>{t('settings.hooksAllAgents')}</SelectItem>
              {agents.map((item) => (
                <SelectItem key={item} value={item}>
                  {item}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <div className="relative min-w-48 flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={keyword}
              onChange={(event) => setKeyword(event.target.value)}
              placeholder={t('settings.hooksKeywordPlaceholder')}
              aria-label={t('settings.hooksFilterKeyword')}
              className="h-9 pl-8"
            />
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void load()}
            disabled={loading}
            aria-label={t('settings.hooksRefresh')}
          >
            <RefreshCw className={cn('h-3.5 w-3.5', loading && 'animate-spin')} />
          </Button>
        </div>

        {loading ? (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {t('settings.hooksLoading')}
          </div>
        ) : loadError ? (
          <div className="alert-danger rounded-lg border px-4 py-3 text-sm">
            {t('settings.hooksError', { error: loadError })}
          </div>
        ) : filtered.length === 0 ? (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {entries.length === 0 ? t('settings.hooksEmpty') : t('settings.hooksNoMatch')}
          </div>
        ) : (
          <>
            <div className="max-h-[min(62vh,34rem)] overflow-y-auto rounded-lg border border-border/60 px-2">
              <ul className="divide-y divide-border/70">
                {filtered.map((entry) => (
                  <HookRow
                    key={entry.seq}
                    entry={entry}
                    language={language}
                    t={t}
                    expanded={expanded === entry.seq}
                    onToggle={() => setExpanded(expanded === entry.seq ? null : entry.seq)}
                  />
                ))}
              </ul>
            </div>
            <p className="text-center text-xs text-muted-foreground">
              {t('settings.hooksCount', { total: entries.length, shown: filtered.length })}
            </p>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}