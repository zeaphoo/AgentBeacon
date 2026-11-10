import { describe, expect, it } from 'vitest'
import { agentLogoId } from './agents'

describe('agentLogoId', () => {
  it('maps aliases to bundled logo ids', () => {
    expect(agentLogoId('claude')).toBe('claude-code')
    expect(agentLogoId('GitHub Copilot')).toBe('github-copilot')
    expect(agentLogoId('Grok Build')).toBe('grok')
    expect(agentLogoId('pi')).toBe('pi')
  })

  it('keeps unknown agents for the generic fallback', () => {
    expect(agentLogoId('future-agent')).toBe('future-agent')
  })
})
