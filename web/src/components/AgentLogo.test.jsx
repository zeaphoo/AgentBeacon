import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import AgentLogo from './AgentLogo'

describe('AgentLogo', () => {
  it('renders a bundled image for bitmap brands', () => {
    const { container } = render(<AgentLogo id="qoder" className="h-6 w-6" />)
    const image = container.querySelector('img')
    expect(image).toBeTruthy()
    expect(image.getAttribute('src')).toContain('qoder')
    expect(image.getAttribute('aria-hidden')).toBe('true')
  })

  it.each(['codebuddy', 'workbuddy', 'hermes', 'zcode'])(
    'renders an official bundled asset for %s',
    (id) => {
      const { container } = render(<AgentLogo id={id} className="h-6 w-6" />)
      const image = container.querySelector('img')
      expect(image).toBeTruthy()
      expect(image.getAttribute('src')).toContain(id)
      expect(image.getAttribute('aria-hidden')).toBe('true')
    },
  )

  it('renders the GitHub Copilot brand mark', () => {
    const { container } = render(<AgentLogo id="github-copilot" className="h-6 w-6" />)
    const mark = container.querySelector('[data-agent-logo="github-copilot"] svg')
    expect(mark).toBeTruthy()
    expect(mark.getAttribute('fill')).toBe('currentColor')
  })

  it('falls back to a generic icon for unknown integrations', () => {
    const { container } = render(<AgentLogo id="future-agent" className="h-6 w-6" />)
    expect(container.querySelector('[data-agent-logo="future-agent"] svg')).toBeTruthy()
  })
})
