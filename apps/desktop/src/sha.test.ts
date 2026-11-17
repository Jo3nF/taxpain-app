import { describe, expect, it } from 'vitest';
import { sha256Hex } from './api';

describe('sha256Hex', () => {
  it('coincide con el vector de prueba y con la huella de la auditoría (UTF-8)', async () => {
    expect(await sha256Hex('abc')).toBe(
      'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    );
    expect(await sha256Hex('Ñ')).toHaveLength(64);
  });
});
