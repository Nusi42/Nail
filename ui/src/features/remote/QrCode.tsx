/**
 * A QR code as SVG (matrix from `uqr`, MIT). The dark modules are one
 * path (`remote-qr.modules`), the light background one rectangle
 * (`remote-qr.background`); the theme colours both.
 */
import { useMemo } from "react";
import { encode } from "uqr";

/** SVG path data of the dark modules of `data`'s QR code (with a quiet zone). */
export function qrPath(data: string): { size: number; path: string } {
  const qr = encode(data, { ecc: "M", border: 4 });
  const parts: string[] = [];
  qr.data.forEach((row, y) => {
    row.forEach((dark, x) => {
      if (dark) parts.push(`M${x} ${y}h1v1h-1z`);
    });
  });
  return { size: qr.size, path: parts.join("") };
}

export function QrCode({ data, label }: { data: string; label: string }) {
  const { size, path } = useMemo(() => qrPath(data), [data]);
  return (
    <svg
      data-part="remote-qr"
      role="img"
      aria-label={label}
      viewBox={`0 0 ${size} ${size}`}
      shapeRendering="crispEdges"
    >
      <rect data-part="remote-qr.background" width={size} height={size} />
      <path data-part="remote-qr.modules" d={path} />
    </svg>
  );
}
