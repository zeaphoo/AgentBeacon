import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import HookLogDialog from './HookLogDialog'

const now = Date.now()

const payload = {
  capacity: 2000,
  next_seq: 3,
  entries: [
    {
      seq: 1,
      at_ms: now - 120_000,
      kind: 'event',
      agent: 'pi',
      session: 'session-1',
      state: 'working',
      message: 'bash',
      outcome: 'applied',
      aggregate_before: 'idle',
      aggregate_after: 'working',
      effect_before: 'calm',
      effect_after: 'busy',
    },
    {
      seq: 2,
      at_ms: now - 60_000,
      kind: 'event',
      agent: 'codex',
      session: 'session-2',
      state: 'waiting',
      outcome: 'applied',
      aggregate_before: 'working',
      aggregate_after: 'waiting',
      effect_before: 'busy',
      effect_after: 'attention',
    },
    {
      seq: 3,
      at_ms: now - 1_000,
      kind: 'config',
      outcome: 'rejected',
      error: 'unknown variant `busy`',
      aggregate_before: 'waiting',
      aggregate_after: 'waiting',
      effect_before: 'attention',
      effect_after: 'attention',
    },
  ],
}

function mockFetch() {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    text: async () => JSON.stringify(payload),
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

describe('HookLogDialog', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('loads the in-memory log when opened', async () => {
    const user = userEvent.setup()
    const fetchMock = mockFetch()
    render(<HookLogDialog />)

    expect(fetchMock).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: '查看事件日志' }))

    expect(await screen.findByText('事件日志')).toBeInTheDocument()
    expect(fetchMock.mock.calls[0][0]).toBe('/api/v1/hooks?limit=2000')
    expect(screen.getByText(/session-1/)).toBeInTheDocument()
    expect(screen.getByText(/session-2/)).toBeInTheDocument()
    expect(screen.getByText('共 3 条，筛选后 3 条')).toBeInTheDocument()
    expect(screen.getByText('配置变更')).toBeInTheDocument()
    expect(screen.getByText('已拒绝')).toBeInTheDocument()
  })

  it('filters by keyword and expands details', async () => {
    const user = userEvent.setup()
    mockFetch()
    render(<HookLogDialog />)
    await user.click(screen.getByRole('button', { name: '查看事件日志' }))
    await screen.findByText(/session-1/)

    await user.type(screen.getByLabelText('关键词'), 'codex')
    expect(screen.getByText(/session-2/)).toBeInTheDocument()
    expect(screen.queryByText(/session-1/)).not.toBeInTheDocument()
    expect(screen.getByText('共 3 条，筛选后 1 条')).toBeInTheDocument()

    await user.clear(screen.getByLabelText('关键词'))
    await user.type(screen.getByLabelText('关键词'), 'unknown variant')
    expect(screen.getByText('配置变更')).toBeInTheDocument()
    expect(screen.queryByText(/session-1/)).not.toBeInTheDocument()

    await user.clear(screen.getByLabelText('关键词'))
    const row = screen.getByText(/session-1/).closest('button')
    await user.click(row)
    expect(screen.getByText('聚合状态')).toBeInTheDocument()
    expect(screen.getAllByText('空闲 → 运行中')).toHaveLength(2)
    expect(screen.getByText('#1')).toBeInTheDocument()
  })
})