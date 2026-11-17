import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDemoInvoke } from './api';
import { Contacts } from './Contacts';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const row = (over: Record<string, unknown>) => ({
  key: 'B12345674',
  id: null,
  role: 'cliente',
  name: 'Cliente SL',
  tax_id: 'B12345674',
  country: 'ES',
  email: null,
  invoiced_year: 121_000,
  invoiced_total: 121_000,
  purchased_year: 0,
  purchased_total: 0,
  to_collect: 0,
  to_pay: 0,
  overdue: 0,
  last_date: '2026-10-01',
  invoices: 1,
  ...over,
});

describe('Contacts', () => {
  it('filtra, abre la ficha y guarda un cliente de la UE con su NIF-IVA', async () => {
    const saved: unknown[] = [];
    setDemoInvoke(async (cmd, args) => {
      if (cmd === 'contacts_list')
        return [
          row({}),
          row({
            key: 'FR12345678901',
            name: 'Atelier SAS',
            tax_id: 'FR12345678901',
            country: 'FR',
          }),
          row({ key: 'B00654327', name: 'Proveedor SL', role: 'proveedor', tax_id: 'B00654327' }),
        ];
      if (cmd === 'contact_detail')
        return {
          row: row({
            key: 'FR12345678901',
            name: 'Atelier SAS',
            tax_id: 'FR12345678901',
            country: 'FR',
          }),
          data: null,
          party: null,
          history: [],
          years: [],
        };
      if (cmd === 'contact_save') {
        saved.push(args?.input);
        return 5;
      }
      return null;
    });
    render(<Contacts now={new Date(2026, 9, 24)} />);
    expect(await screen.findByText('Atelier SAS')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Proveedores' }));
    expect(screen.queryByText('Atelier SAS')).toBeNull();
    expect(screen.getByText('Proveedor SL')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Todos' }));
    fireEvent.change(screen.getByPlaceholderText('Nombre o NIF'), { target: { value: 'fr123' } });
    fireEvent.click(screen.getByRole('button', { name: 'Atelier SAS' }));
    expect(await screen.findByText(/Francia/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Crear ficha' }));
    fireEvent.change(screen.getByLabelText('Correo para las facturas'), {
      target: { value: 'compta@atelier.example' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar ficha' }));
    await screen.findByRole('button', { name: 'Editar ficha' }).catch(() => null);
    expect(saved[0]).toMatchObject({
      id: null,
      role: 'cliente',
      data: {
        party: { id: { Foreign: { country: 'FR', id_type: 'NifIva', id: 'FR12345678901' } } },
        email: 'compta@atelier.example',
      },
    });
  });

  it('comprobar en VIES un proveedor lo deja como proveedor', async () => {
    const vies: unknown[] = [];
    const supplier = row({
      key: 'DE123456789',
      name: 'Lieferant GmbH',
      role: 'proveedor',
      tax_id: 'DE123456789',
      country: 'DE',
    });
    setDemoInvoke(async (cmd, args) => {
      if (cmd === 'contacts_list') return [supplier];
      if (cmd === 'contact_detail')
        return { row: supplier, data: null, party: null, history: [], years: [] };
      if (cmd === 'vies_check') {
        vies.push(args);
        return { valid: true };
      }
      return null;
    });
    render(<Contacts now={new Date(2026, 9, 24)} initialKey="DE123456789" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Comprobar en VIES' }));
    await vi.waitFor(() => expect(vies).toHaveLength(1));
    expect(vies[0]).toMatchObject({ country: 'DE', vat: 'DE123456789', role: 'proveedor' });
  });

  it('tras cambiar el NIF o borrar la ficha, el panel no se queda en una clave que ya no existe', async () => {
    let rows = [row({ id: 3, key: 'B12345674' })];
    const detailOf = (key: string) => {
      const r = rows.find((x) => x.key === key);
      if (!r) throw { message: 'no encontrado' };
      return { row: r, data: null, party: null, history: [], years: [] };
    };
    setDemoInvoke(async (cmd, args) => {
      if (cmd === 'contacts_list') return rows;
      if (cmd === 'contact_detail') return detailOf(String(args?.key));
      if (cmd === 'contact_save') {
        // El NIF cambia: la clave de la fila, también.
        rows = [row({ id: 3, key: 'A22222228', tax_id: 'A22222228', name: 'Cliente Nuevo SA' })];
        return 3;
      }
      if (cmd === 'contact_delete') {
        rows = [];
        return null;
      }
      return null;
    });
    render(<Contacts now={new Date(2026, 9, 24)} initialKey="B12345674" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Editar ficha' }));
    fireEvent.change(screen.getByLabelText('NIF'), { target: { value: 'A22222228' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar ficha' }));
    expect(await screen.findByRole('heading', { name: 'Cliente Nuevo SA' })).toBeTruthy();
    expect(screen.queryByText('no encontrado')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Borrar ficha' }));
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Borrar ficha' }),
    );
    await vi.waitFor(() =>
      expect(screen.queryByRole('heading', { name: 'Cliente Nuevo SA' })).toBeNull(),
    );
    expect(screen.queryByText('no encontrado')).toBeNull();
  });

  it('un contacto nuevo con un NIF que ya tiene ficha muestra el error', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'contacts_list') return [];
      if (cmd === 'contact_save')
        throw {
          message: 'ya hay una ficha con ese NIF (B12345674): búscala en la lista y edítala',
        };
      return null;
    });
    render(<Contacts now={new Date(2026, 9, 24)} />);
    fireEvent.click(await screen.findByRole('button', { name: /Nuevo contacto/ }));
    fireEvent.change(screen.getByLabelText('Nombre o razón social'), {
      target: { value: 'Cliente SL' },
    });
    fireEvent.change(screen.getByLabelText('NIF'), { target: { value: 'B12345674' } });
    fireEvent.click(screen.getByRole('button', { name: 'Guardar ficha' }));
    expect((await screen.findByRole('alert')).textContent).toContain(
      'ya hay una ficha con ese NIF',
    );
  });
});
