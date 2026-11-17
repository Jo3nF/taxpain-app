import { describe, expect, it } from 'vitest';
import { valoresHandlers } from './demoValores';
import type { CuentaView, OpValores, ValoresReport } from './valores';

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

describe('demo de Inversiones en valores', () => {
  it('cuadra: totales, ventas con su operación, cuentas, 720 y asientos', () => {
    const r = valoresHandlers.valores_report?.({ year: 2026 }) as ValoresReport;
    const ops = valoresHandlers.valores_ops?.({ year: null }) as OpValores[];
    expect(r.coste_total).toBe(sum(r.posiciones.map((p) => p.coste)));
    expect(r.valor_total).toBe(sum(r.posiciones.map((p) => p.valor ?? 0)));
    expect(r.latente_total).toBe(sum(r.posiciones.map((p) => p.latente ?? 0)));
    for (const p of r.posiciones) expect(p.latente).toBe((p.valor ?? 0) - p.coste);
    for (const x of r.realizados) {
      const o = ops.find((o) => o.id === x.op);
      expect(o?.kind).toBe('venta');
      expect(o?.qty).toBe(x.cantidad);
      expect(x.resultado).toBe(x.transmision - x.coste);
    }
    expect(r.resultado.ventas).toBe(r.realizados.length);
    expect(r.resultado.neto).toBe(sum(r.realizados.map((x) => x.resultado)));
    // Títulos de cada posición = compras − ventas.
    for (const p of r.posiciones) {
      const q = sum(
        ops
          .filter((o) => o.security_id === p.security_id)
          .map((o) => (o.kind === 'compra' ? 1 : o.kind === 'venta' ? -1 : 0) * Number(o.qty)),
      );
      expect(String(q)).toBe(p.cantidad);
    }
    for (const c of r.cuentas as CuentaView[])
      expect(c.ops).toBe(ops.filter((o) => o.account_id === c.id).length);
    expect(r.m720.total_extranjero).toBe(sum(r.m720.cuentas.map((c) => c.valor)));
    for (const a of r.asientos)
      expect(sum(a.lineas.map((l) => l.debe))).toBe(sum(a.lineas.map((l) => l.haber)));
    // Otro año: vacío.
    const otro = valoresHandlers.valores_report?.({ year: 2025 }) as ValoresReport;
    expect(otro.posiciones).toHaveLength(0);
  });
});
