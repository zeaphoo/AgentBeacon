import { fireEvent, render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import DeviceImage, { deviceImageScale } from './DeviceImage'

describe('DeviceImage', () => {
  it('renders the bundled image for a known profile', () => {
    const { container } = render(<DeviceImage profile="agentcore-light" className="h-12 w-12" />)
    const image = container.querySelector('img')
    expect(image).toBeTruthy()
    expect(image.getAttribute('src')).toContain('agentcore-light')
    expect(image.getAttribute('aria-hidden')).toBe('true')
  })

  it('reuses the Halo image for mlight-3led', () => {
    const halo = render(<DeviceImage profile="agentcore-halo" />).container.querySelector('img')
    const mlight = render(<DeviceImage profile="mlight-3led" />).container.querySelector('img')
    expect(mlight.getAttribute('src')).toBe(halo.getAttribute('src'))
    expect(deviceImageScale('mlight-3led')).toBe(deviceImageScale('agentcore-halo'))
  })

  it('scales non-square assets to balance their visual size', () => {
    const { container } = render(<DeviceImage profile="agentcore-light" />)
    const image = container.querySelector('img')
    expect(image.style.transform).toBe('scale(1.12)')
    expect(deviceImageScale('agentcore-light')).toBeGreaterThan(deviceImageScale('agentcore-halo'))
  })

  it('falls back to a generic icon for devices without a picture', () => {
    const { container } = render(<DeviceImage profile="yh50-rgy-j" className="h-12 w-12" />)
    const fallback = container.querySelector('[data-device-image="fallback"]')
    expect(fallback).toBeTruthy()
    expect(fallback.querySelector('svg')).toBeTruthy()
    expect(container.querySelector('img')).toBeNull()
  })

  it('falls back to a generic icon when images are disabled', () => {
    const { container } = render(
      <DeviceImage profile="agentcore-light" showImage={false} className="h-12 w-12" />,
    )
    expect(container.querySelector('[data-device-image="fallback"]')).toBeTruthy()
    expect(container.querySelector('img')).toBeNull()
  })

  it('falls back when the image fails to load', () => {
    const { container } = render(<DeviceImage profile="agentcore-light" />)
    fireEvent.error(container.querySelector('img'))
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('[data-device-image="fallback"]')).toBeTruthy()
  })

  it('dims the image when the device is disconnected', () => {
    const { container } = render(<DeviceImage profile="agentcore-halo" connected={false} />)
    const image = container.querySelector('img')
    expect(image.className).toContain('opacity-50')
    expect(image.className).toContain('grayscale')
  })
})
