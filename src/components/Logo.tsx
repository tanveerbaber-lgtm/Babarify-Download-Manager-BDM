import { useId } from "react";

interface LogoProps {
  size?: number;
  title?: string;
  /** Draw the mark on the navy app-icon tile (for light backgrounds). */
  tile?: boolean;
}

/** Babarify "B" mark: blue-to-green B with a download arrow (see assets/brand). */
export function Logo({ size = 32, title = "Babarify Download Manager", tile = false }: LogoProps) {
  const id = useId().replace(/:/g, "");
  const b = `bdm-b-${id}`;
  const shade = `bdm-s-${id}`;
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label={title}>
      <defs>
        <linearGradient id={b} x1="0.05" y1="0.9" x2="0.95" y2="0.15">
          <stop offset="0" stopColor="#2563EB" />
          <stop offset="0.4" stopColor="#1F78E6" />
          <stop offset="0.9" stopColor="#10B981" />
        </linearGradient>
        <linearGradient id={shade} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#0F172A" stopOpacity="0" />
          <stop offset="1" stopColor="#0F172A" stopOpacity="0.18" />
        </linearGradient>
      </defs>
      {tile && <rect width="64" height="64" rx="14" fill="#0F172A" />}
      <g transform={tile ? "translate(9 9) scale(0.72)" : undefined}>
        <path
          fill={`url(#${b})`}
          d="M15 5h19.5C45 5 51 11.2 51 19.6c0 4.6-2.2 8.4-5.8 10.6C51.2 32.3 56 37.9 56 45.2 56 53.4 49.4 59 39.6 59H15c-4.4 0-7-2.6-7-7V12c0-4.4 2.6-7 7-7z"
        />
        <path
          fill={`url(#${shade})`}
          opacity="0.6"
          d="M30 5h4.5C45 5 51 11.2 51 19.6c0 4.6-2.2 8.4-5.8 10.6C51.2 32.3 56 37.9 56 45.2 56 53.4 49.4 59 39.6 59H30z"
        />
        <path
          fill="#fff"
          d="M18 15.5a3 3 0 0 1 3-3h4a3 3 0 0 1 3 3V35h5.6c1.9 0 2.8 2.3 1.5 3.6L24.6 49.2a2.3 2.3 0 0 1-3.2 0L10.9 38.6c-1.3-1.3-.4-3.6 1.5-3.6H18z"
        />
      </g>
    </svg>
  );
}

export const TAGLINE = "Faster Downloads. More Possibilities.";
