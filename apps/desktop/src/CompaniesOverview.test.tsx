import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setDemoInvoke } from './api';
import { CompaniesOverview } from './CompaniesOverview';

afterEach(() => {
  cleanup();
  setDemoInvoke(null);
});

const company = (id: string, active: boolean, checked: string | null = null) => ({
  id,
  name: `EMPRESA ${id}`,
  nif: null,
  active,
  status: {
    aeat_checked_at: checked,
    has_profile: true,
    overdue: [],
    upcoming: [
      {
        model: 'cuota-autonomos',
        label: 'Cuota de autónomos · octubre 2026',
        description: 'Cuota',
        period: '10',
        fiscal_year: 2026,
        due_on: '2026-10-31',
        filed: null,
      },
    ],
  },
});

describe('CompaniesOverview', () => {
  it('enseña los plazos con su título legible, no el código interno', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'companies_overview') return [company('A', true), company('B', false)];
      throw { message: `sin datos: ${cmd}` };
    });
    render(<CompaniesOverview />);
    expect((await screen.findAllByText(/Cuota de autónomos · octubre 2026/)).length).toBe(2);
    expect(screen.queryByText(/cuota-autonomos/)).toBeNull();
  });

  it('dice de cada empresa si se comprobó con la AEAT y ofrece comprobar la abierta', async () => {
    setDemoInvoke(async (cmd) => {
      if (cmd === 'companies_overview')
        return [company('A', true), company('B', false, '2026-10-27T18:30:00Z')];
      throw { message: `sin datos: ${cmd}` };
    });
    let checked = false;
    render(
      <CompaniesOverview
        onCheck={() => {
          checked = true;
        }}
      />,
    );
    expect(await screen.findByText('Sin comprobar con la AEAT')).toBeTruthy();
    expect(screen.getByText('Comprobado con la AEAT el 27/10/2026')).toBeTruthy();
    expect(screen.getByText(/EMPRESA A no se ha comprobado con la AEAT/)).toBeTruthy();
    screen.getByRole('button', { name: 'Comprobar la empresa abierta' }).click();
    expect(checked).toBe(true);
    expect(screen.queryByText(/no hagas caso del aviso/)).toBeNull();
  });
});
