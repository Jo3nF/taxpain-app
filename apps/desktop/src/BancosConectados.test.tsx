import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BancaActualizacion, BancaEstado } from './api';

const api = vi.hoisted(() => ({
  bancaEstado: vi.fn(),
  bancaAjustes: vi.fn(),
  bancaCredenciales: vi.fn(),
  bancaQuitarCredenciales: vi.fn(),
  bancaBancos: vi.fn(),
  bancaConectar: vi.fn(),
  bancaEsperar: vi.fn(),
  bancaTerminar: vi.fn(),
  bancaCancelar: vi.fn(),
  bancaActualizar: vi.fn(),
  bancaActivarCuenta: vi.fn(),
  bancaDesconectar: vi.fn(),
  pickPemFile: vi.fn(),
  openSede: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const { BancosConectados, describeActualizacion } = await import('./BancosConectados');

afterEach(() => {
  cleanup();
  for (const f of Object.values(api)) f.mockReset();
});

const SIN_CONFIGURAR: BancaEstado = {
  ajustes: { aceptado: false, url_vuelta: 'https://localhost/taxpain/banco' },
  app_id: null,
  clave: false,
  vuelta_local: false,
  conexiones: [],
  pendiente: null,
};

const CONECTADO: BancaEstado = {
  ...SIN_CONFIGURAR,
  ajustes: { aceptado: true, url_vuelta: 'https://localhost/taxpain/banco' },
  app_id: '0a1b2c3d-0000-4000-8000-00000000a1b2',
  clave: true,
  conexiones: [
    {
      id: 1,
      banco: 'BBVA',
      pais: 'ES',
      tipo: 'business',
      estado: 'activa',
      hasta: '2027-04-21T09:30:00Z',
      dias: 172,
      actualizada: '2026-10-30T08:05:00Z',
      error: null,
      cuentas: [
        {
          id: 1,
          nombre: 'EMPRESA DEMO SL',
          iban: 'ES91 …1332',
          divisa: 'EUR',
          tarjeta: false,
          activa: true,
          cuenta_id: 1,
          subcuenta: '5720001',
          hasta_dia: '2026-10-30',
          saldo: 214_950,
          saldo_fecha: '2026-10-29',
        },
      ],
    },
  ],
};

const ACTUALIZADO: BancaActualizacion = {
  movimientos_nuevos: 3,
  ya_estaban: 1,
  cuentas: [
    {
      conexion: 1,
      banco: 'BBVA',
      nombre: 'ES91 …1332',
      error: null,
      resultado: {
        cuenta: 'BBVA …1332 EUR',
        cuenta_id: 1,
        cuenta_nueva: false,
        subcuenta: '5720001',
        formato: 'Conexión con el banco',
        clase: 'Cuenta',
        divisa: 'EUR',
        desde: '2026-10-02',
        hasta: '2026-10-06',
        movimientos: 4,
        movimientos_nuevos: 3,
        ya_estaban: 1,
        saldos_completados: 0,
        ya_contabilizados: 0,
        cuadre: { estado: 'sin_saldos' },
        importado: true,
        motivo: '',
        avisos: ['Posible duplicado: el 2026-10-06 entra «CAFE» (2,50 €)…'],
      },
    },
    {
      conexion: 2,
      banco: 'Revolut',
      nombre: 'Main',
      error: 'el permiso ha caducado',
      resultado: null,
    },
  ],
};

describe('BancosConectados', () => {
  it('describe cada cuenta descargada', () => {
    expect(describeActualizacion(ACTUALIZADO)).toEqual([
      {
        linea: 'BBVA ES91 …1332: 3 movimientos nuevos (1 ya estaba)',
        avisos: ['Posible duplicado: el 2026-10-06 entra «CAFE» (2,50 €)…'],
      },
      { linea: 'Revolut Main: el permiso ha caducado', avisos: [] },
    ]);
    expect(describeActualizacion({ cuentas: [], movimientos_nuevos: 0, ya_estaban: 0 })).toEqual([
      { linea: 'No hay ninguna cuenta activa que descargar.', avisos: [] },
    ]);
  });

  it('sin configurar: guarda la clave con el id del nombre del fichero y acepta', async () => {
    api.bancaEstado.mockResolvedValue(SIN_CONFIGURAR);
    api.pickPemFile.mockResolvedValue(
      '/Users/demo/Downloads/0a1b2c3d-0000-4000-8000-00000000a1b2.pem',
    );
    api.bancaCredenciales.mockResolvedValue({
      ...SIN_CONFIGURAR,
      clave: true,
      app_id: '0a1b2c3d-0000-4000-8000-00000000a1b2',
    });
    api.bancaAjustes.mockResolvedValue({ ...SIN_CONFIGURAR, clave: true });
    render(<BancosConectados onImported={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Elegir el fichero .pem…' }));
    await waitFor(() =>
      expect(api.bancaCredenciales).toHaveBeenCalledWith(
        '0a1b2c3d-0000-4000-8000-00000000a1b2',
        '/Users/demo/Downloads/0a1b2c3d-0000-4000-8000-00000000a1b2.pem',
      ),
    );
    expect(await screen.findByText(/no va en las copias de seguridad/)).toBeTruthy();
    fireEvent.click(screen.getByRole('checkbox', { name: /Acepto que TaxPain/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Guardar' }));
    await waitFor(() =>
      expect(api.bancaAjustes).toHaveBeenCalledWith({
        aceptado: true,
        url_vuelta: 'https://localhost/taxpain/banco',
      }),
    );
    // Sin aceptar todavía, no se ofrece conectar.
    expect(screen.queryByRole('button', { name: 'Conectar un banco…' })).toBeNull();
  });

  it('enseña las conexiones y actualiza los movimientos', async () => {
    api.bancaEstado.mockResolvedValue(CONECTADO);
    api.bancaActualizar.mockResolvedValue(ACTUALIZADO);
    const onImported = vi.fn();
    render(<BancosConectados onImported={onImported} />);
    expect(await screen.findByText('BBVA · empresa')).toBeTruthy();
    expect(screen.getByText(/Permiso hasta el 21\/04\/2027 \(quedan 172 días\)/)).toBeTruthy();
    expect(screen.getByText(/Saldo en el banco: 2\.149,50 €/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Actualizar movimientos' }));
    expect(
      await screen.findByText('BBVA ES91 …1332: 3 movimientos nuevos (1 ya estaba)'),
    ).toBeTruthy();
    // Los avisos de la importación se ven, destacados los que cambian lo que ha entrado.
    expect(screen.getByText(/Posible duplicado/).className).toContain('amber');
    expect(api.bancaActualizar).toHaveBeenCalledWith(null);
    expect(onImported).toHaveBeenCalled();
  });

  it('conecta un banco pegando la dirección de vuelta', async () => {
    api.bancaEstado.mockResolvedValue(CONECTADO);
    api.bancaBancos.mockResolvedValue([
      {
        name: 'BBVA',
        country: 'ES',
        psu_types: ['business', 'personal'],
        maximum_consent_validity: 15_552_000,
        beta: false,
      },
      {
        name: 'Revolut',
        country: 'ES',
        psu_types: ['business'],
        maximum_consent_validity: null,
        beta: false,
      },
    ]);
    api.bancaConectar.mockResolvedValue({ escucha: false });
    api.bancaTerminar.mockResolvedValue(ACTUALIZADO);
    render(<BancosConectados onImported={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Conectar un banco…' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Revolut · empresa' }));
    await waitFor(() =>
      expect(api.bancaConectar).toHaveBeenCalledWith('Revolut', 'ES', 'business', null),
    );
    const campo = await screen.findByLabelText('Dirección a la que te ha llevado el banco');
    expect(screen.getByRole('button', { name: 'Terminar la conexión' })).toHaveProperty(
      'disabled',
      true,
    );
    fireEvent.change(campo, {
      target: { value: 'https://localhost/taxpain/banco?state=abc&code=xyz' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Terminar la conexión' }));
    await waitFor(() =>
      expect(api.bancaTerminar).toHaveBeenCalledWith(
        'https://localhost/taxpain/banco?state=abc&code=xyz',
      ),
    );
    expect(await screen.findByText(/3 movimientos nuevos/)).toBeTruthy();
    expect(api.bancaEsperar).not.toHaveBeenCalled();
  });

  it('si TaxPain escucha la vuelta, la recoge sola', async () => {
    api.bancaEstado.mockResolvedValue({
      ...CONECTADO,
      ajustes: { aceptado: true, url_vuelta: 'http://127.0.0.1:47219/taxpain/banco' },
      vuelta_local: true,
    });
    api.bancaBancos.mockResolvedValue([
      {
        name: 'BBVA',
        country: 'ES',
        psu_types: ['business'],
        maximum_consent_validity: null,
        beta: false,
      },
    ]);
    api.bancaConectar.mockResolvedValue({ escucha: true });
    api.bancaEsperar.mockResolvedValue(ACTUALIZADO);
    render(<BancosConectados onImported={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Conectar un banco…' }));
    fireEvent.click(await screen.findByRole('button', { name: 'BBVA · empresa' }));
    expect(await screen.findByText(/3 movimientos nuevos/)).toBeTruthy();
    expect(api.bancaEsperar).toHaveBeenCalled();
  });

  it('un error al conectar se enseña', async () => {
    api.bancaEstado.mockResolvedValue(CONECTADO);
    api.bancaBancos.mockRejectedValue({ message: 'no se ha podido conectar con Enable Banking' });
    render(<BancosConectados onImported={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Conectar un banco…' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Enable Banking');
  });
});

describe('BancosConectados: quitar y esperar', () => {
  it('quitar la conexión enseña el aviso si hay que retirarla en el banco', async () => {
    api.bancaEstado.mockResolvedValue(CONECTADO);
    api.bancaDesconectar.mockResolvedValue({
      estado: { ...CONECTADO, conexiones: [] },
      aviso: 'Sin la clave de la aplicación no se ha podido retirar el permiso…',
    });
    render(<BancosConectados onImported={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Quitar la conexión con BBVA' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Quitar' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Sin la clave');
    expect(screen.getByText('Todavía no hay ningún banco conectado.')).toBeTruthy();
  });

  it('si se deja de esperar (dirección pegada), no sale ningún error', async () => {
    api.bancaEstado.mockResolvedValue({
      ...CONECTADO,
      ajustes: { aceptado: true, url_vuelta: 'http://127.0.0.1:47219/taxpain/banco' },
      vuelta_local: true,
    });
    api.bancaBancos.mockResolvedValue([
      {
        name: 'BBVA',
        country: 'ES',
        psu_types: ['business'],
        maximum_consent_validity: null,
        beta: false,
      },
    ]);
    api.bancaConectar.mockResolvedValue({ escucha: true });
    let soltar: (v: null) => void = () => {};
    api.bancaEsperar.mockReturnValue(
      new Promise((r) => {
        soltar = r;
      }),
    );
    api.bancaTerminar.mockResolvedValue(ACTUALIZADO);
    render(<BancosConectados onImported={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Conectar un banco…' }));
    fireEvent.click(await screen.findByRole('button', { name: 'BBVA · empresa' }));
    const campo = await screen.findByLabelText('Dirección a la que te ha llevado el banco');
    fireEvent.change(campo, {
      target: { value: 'http://127.0.0.1:47219/taxpain/banco?state=abc&code=xyz' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Terminar la conexión' }));
    expect(await screen.findByText(/3 movimientos nuevos/)).toBeTruthy();
    soltar(null);
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
  });
});
