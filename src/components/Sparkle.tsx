interface SparkleProps {
  size?: number;
  className?: string;
}

/**
 * The Anthropic "sparkle" / burst glyph used throughout the Claude Code
 * desktop client. Rendered as a set of tapered rays radiating from the
 * centre, in the Claude rust-orange brand colour.
 */
export function Sparkle({ size = 18, className }: SparkleProps) {
  const rays = 12;
  const cx = 12;
  const cy = 12;
  const inner = 1.6;
  const outer = 11;
  const halfWidth = 1.25;

  const paths = Array.from({ length: rays }, (_, i) => {
    const a = (Math.PI * 2 * i) / rays;
    const perp = a + Math.PI / 2;
    const tipX = cx + Math.cos(a) * outer;
    const tipY = cy + Math.sin(a) * outer;
    const b1x = cx + Math.cos(a) * inner + Math.cos(perp) * halfWidth;
    const b1y = cy + Math.sin(a) * inner + Math.sin(perp) * halfWidth;
    const b2x = cx + Math.cos(a) * inner - Math.cos(perp) * halfWidth;
    const b2y = cy + Math.sin(a) * inner - Math.sin(perp) * halfWidth;
    return `M${b1x.toFixed(2)} ${b1y.toFixed(2)} L${tipX.toFixed(2)} ${tipY.toFixed(2)} L${b2x.toFixed(2)} ${b2y.toFixed(2)} Z`;
  }).join(' ');

  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
    >
      <path d={paths} />
    </svg>
  );
}
