export default function BeaconLogo({ className = 'h-9 w-9' }) {
  return (
    <img
      src="/logo.svg"
      alt=""
      aria-hidden="true"
      draggable="false"
      className={className}
    />
  )
}
