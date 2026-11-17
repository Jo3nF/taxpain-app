import { useId } from 'react';

/**
 * Logo de TaxPain: una «T» cuyo palo inclinado, con los dos puntos, forma un «%» (impuestos). Va
 * con los colores de España: cuadrado rojo y los puntos del «%» en gualda. Diseño propio; el mismo
 * dibujo está en `apps/desktop/src-tauri/icons/logo.svg` (iconos de Windows y Linux:
 * `npx tauri icon`) y en `logo-macos.svg` (el `icon.icns`, con el margen que usa macOS).
 */
export function LogoMark({ size = 32, title }: { size?: number; title?: string }) {
  const id = `tp-logo-${useId().replace(/:/g, '')}`;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 512 512"
      role={title ? 'img' : undefined}
      aria-hidden={title ? undefined : true}
      aria-label={title}
    >
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#d01c29" />
          <stop offset="1" stopColor="#a8101c" />
        </linearGradient>
      </defs>
      <rect width="512" height="512" rx="116" fill={`url(#${id})`} />
      <rect x="124" y="124" width="264" height="58" rx="29" fill="#fff" />
      <path d="M296 182 L226 392" stroke="#fff" strokeWidth="58" strokeLinecap="round" />
      <circle cx="168" cy="262" r="30" fill="#fabd00" />
      <circle cx="352" cy="346" r="30" fill="#fabd00" />
    </svg>
  );
}

/** Banderita de España (rojo, gualda y rojo; la franja central doble), sin escudo. */
export function SpainFlag({ className = '' }: { className?: string }) {
  return (
    <svg
      className={`es-flag ${className}`}
      viewBox="0 0 12 8"
      width="15"
      height="10"
      aria-hidden="true"
      preserveAspectRatio="none"
    >
      <rect width="12" height="8" fill="#c60b1e" />
      <rect y="2" width="12" height="4" fill="#ffc400" />
    </svg>
  );
}
