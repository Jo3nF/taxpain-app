import { describe, expect, it } from 'vitest';
import { originLabel } from './api';

describe('originLabel', () => {
  it('nombra cada origen de asiento en español y no llama amortización a lo que no lo es', () => {
    expect(originLabel({ ImpuestoSociedades: 2026 })).toBe('Impuesto sobre Sociedades');
    expect(originLabel({ AplicacionResultado: 2025 })).toBe('Aplicación del resultado');
    expect(originLabel({ Leasing: 3 })).toBe('Leasing');
    expect(originLabel({ Cripto: 12 })).toBe('Criptomonedas');
    expect(originLabel({ Amortizacion: 1 })).toBe('Amortización');
    expect(originLabel('Regularizacion')).toBe('Regularización');
    expect(originLabel('Nomina')).toBe('Nómina');
    expect(originLabel('Cierre')).toBe('Cierre');
    // Un origen nuevo del programa que la interfaz aún no conoce.
    expect(originLabel({ Desconocido: 1 } as never)).toBe('Otro');
  });
});
