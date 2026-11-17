import { describe, expect, it } from 'vitest';
import { PROVINCES, provinceAfterPostCode, provinceOfPostCode } from './provincias';

describe('provincias', () => {
  it('las 52, con su código en orden', () => {
    expect(PROVINCES).toHaveLength(52);
    PROVINCES.forEach(([code], i) => {
      expect(code).toBe(String(i + 1).padStart(2, '0'));
    });
  });

  it('la provincia sale de las dos primeras cifras del código postal', () => {
    expect(provinceOfPostCode('46001')).toBe('Valencia/València');
    expect(provinceOfPostCode(' 28013 ')).toBe('Madrid');
    expect(provinceOfPostCode('07800')).toBe('Illes Balears');
    expect(provinceOfPostCode('52001')).toBe('Melilla');
    expect(provinceOfPostCode('53001')).toBeNull();
    expect(provinceOfPostCode('4600')).toBeNull();
    expect(provinceOfPostCode('75008')).toBeNull(); // París: no es un código español
  });

  it('rellena la provincia sin pisar la que escribió el usuario', () => {
    expect(provinceAfterPostCode('', '46001', '')).toBe('Valencia/València');
    // Cambia el código: la puesta sola se actualiza.
    expect(provinceAfterPostCode('46001', '28013', 'Valencia/València')).toBe('Madrid');
    // La escrita a mano se respeta.
    expect(provinceAfterPostCode('46001', '28013', 'València')).toBe('València');
    // A medio escribir, no cambia nada.
    expect(provinceAfterPostCode('46001', '2801', 'Valencia/València')).toBe('Valencia/València');
  });
});
