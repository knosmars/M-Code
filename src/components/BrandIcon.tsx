/**
 * Meyatu Code brand icon — 100% exact match to original 512px logo.
 * Original source: src-tauri/icons/icon.png (gold #FFC131, cyan #24C8DB).
 *
 * AI thinking animation: gentle whole-icon breathing pulse.
 */

export interface BrandIconProps {
  size?: number;
  className?: string;
  animated?: boolean;
}

export function BrandIcon({ size = 18, className, animated = false }: BrandIconProps) {
  return (
    <div
      style={{ width: size, height: size }}
      className={`${className ?? ''} ${animated ? 'brand-icon--animated' : ''}`.trim() || undefined}
      aria-hidden="true"
    >
      <img
        src="/icon.png"
        alt=""
        style={{
          width: '100%',
          height: '100%',
          objectFit: 'contain',
          display: 'block',
        }}
      />
    </div>
  );
}
