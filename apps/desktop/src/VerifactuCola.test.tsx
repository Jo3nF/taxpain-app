import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { AcercaDe } from './AcercaDe';
import { setDemoInvoke, type VerifactuQueue } from './api';
import { installDemo } from './demo';
import { VerifactuCola } from './VerifactuCola';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const base: VerifactuQueue = {
  entorno: 'pruebas',
  sin_produccion: 'sólo pruebas',
  pendientes: 2,
  sin_respuesta: 0,
  rechazados: 0,
  aceptados_con_errores: 0,
  aceptados_pruebas: 0,
  remitidos: 0,
  espera_segundos: 0,
  proximo_envio: null,
  tiempo_espera: 60,
  incidencia_desde: null,
  fallos_seguidos: 0,
  ultimo_intento: null,
  ultimo_resultado: null,
  ultimo_error: null,
  bloqueada: false,
  aviso: null,
  aviso_modo: null,
};

describe('VerifactuCola', () => {
  it('sin nada que contar no ocupa sitio', async () => {
    setDemoInvoke(async () => base);
    const { container } = render(<VerifactuCola refresh={0} />);
    await Promise.resolve();
    expect(container.textContent).toBe('');
  });

  it('enseña la incidencia y la espera que pide la AEAT', async () => {
    setDemoInvoke(async () => ({
      ...base,
      espera_segundos: 90,
      tiempo_espera: 120,
      proximo_envio: '2026-10-13T08:02:00Z',
      incidencia_desde: '2026-10-13T08:00:00Z',
      aviso: 'No se ha podido remitir a la AEAT: 2 registros sin enviar.',
    }));
    render(<VerifactuCola refresh={0} />);
    expect((await screen.findByRole('alert')).textContent).toContain('2 registros sin enviar');
    expect(screen.getByText(/pide 120 s entre envíos/)).toBeTruthy();
    expect(screen.getByText(/entorno de pruebas/)).toBeTruthy();
  });

  it('tras un rechazo del mensaje, reintentar pide confirmación y desbloquea', async () => {
    const calls: string[] = [];
    let bloqueada = true;
    setDemoInvoke(async (cmd) => {
      calls.push(cmd);
      if (cmd === 'verifactu_queue_reopen') {
        bloqueada = false;
        return null;
      }
      return {
        ...base,
        bloqueada,
        aviso: bloqueada ? 'La AEAT rechazó el último envío entero' : null,
        aviso_modo: bloqueada ? null : 'x',
      };
    });
    render(<VerifactuCola refresh={0} />);
    fireEvent.click(await screen.findByText('Reintentar…'));
    expect(calls).not.toContain('verifactu_queue_reopen');
    fireEvent.click(await screen.findByText('Sí, reintentar'));
    expect(await screen.findByText(/Cola desbloqueada/)).toBeTruthy();
    expect(calls).toContain('verifactu_queue_reopen');
  });

  it('con VERI*FACTU desactivado avisa del modo', async () => {
    setDemoInvoke(async () => ({
      ...base,
      pendientes: 0,
      aviso_modo: 'VERI*FACTU está desactivado.',
    }));
    render(<VerifactuCola refresh={0} />);
    expect(await screen.findByText('VERI*FACTU está desactivado.')).toBeTruthy();
  });
});

describe('AcercaDe', () => {
  it('muestra la declaración responsable con sus apartados y lo que falta', async () => {
    installDemo();
    render(<AcercaDe />);
    expect(
      await screen.findByText('DECLARACIÓN RESPONSABLE DEL SISTEMA INFORMÁTICO DE FACTURACIÓN'),
    ).toBeTruthy();
    expect(screen.getByText(/a\) Nombre del sistema informático/)).toBeTruthy();
    expect(screen.getByText(/l\) Fecha y lugar de la firma/)).toBeTruthy();
    expect(screen.getByText(/falta dirección postal completa del productor/)).toBeTruthy();
    expect(screen.getByText('Guardar en PDF…')).toBeTruthy();
  });
});
