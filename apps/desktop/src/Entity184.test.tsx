import { describe, expect, it } from 'vitest';
import { parsePct4 } from './Entity184';

describe('parsePct4', () => {
  it('pasa el porcentaje a diezmilésimas de punto', () => {
    expect(parsePct4('50')).toBe(500000);
    expect(parsePct4('33,3333')).toBe(333333);
    expect(parsePct4('0')).toBeNull();
    expect(parsePct4('101')).toBeNull();
  });
});
