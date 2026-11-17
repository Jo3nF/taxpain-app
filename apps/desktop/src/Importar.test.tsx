import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AeatCsvItem, CertificateInfo, FolderInventory, FolderItem } from './api';

const api = vi.hoisted(() => ({
  folderGet: vi.fn(),
  folderScan: vi.fn(),
  folderApply: vi.fn(),
  filedList: vi.fn(),
  filedCompare: vi.fn(),
  filedSaveManual: vi.fn(),
  filedDelete: vi.fn(),
  receivedAiFillPending: vi.fn(),
  companyDocsList: vi.fn(),
  companySuggestions: vi.fn(),
  companyApplySuggestion: vi.fn(),
  companyDocAi: vi.fn(),
  companyDocOpen: vi.fn(),
  companyDocDelete: vi.fn(),
  lecturaIaGet: vi.fn(),
  lecturaIaSave: vi.fn(),
  folderAiRead: vi.fn(),
  folderReview: vi.fn(),
  folderReviewLast: vi.fn(),
  aeatCsvDownload: vi.fn(),
  aeatCsvRunning: vi.fn(),
  onAeatCsvDone: vi.fn(),
  aeatCheckTarget: vi.fn(),
  listCertificates: vi.fn(),
  watchedPending: vi.fn(),
  watchedDismiss: vi.fn(),
}));
vi.mock('./api', async (orig) => ({ ...(await orig<typeof import('./api')>()), ...api }));
const {
  Importar,
  describe: describeFound,
  needsAi,
  csvLine,
  mergeCsvResults,
  forgetCsvJob,
} = await import('./Importar');

afterEach(() => {
  cleanup();
  forgetCsvJob();
  for (const f of Object.values(api)) f.mockReset();
});

beforeEach(() => {
  api.lecturaIaGet.mockResolvedValue({ full: false, model: null, accepted_at: null });
  api.aeatCsvRunning.mockResolvedValue(false);
  api.onAeatCsvDone.mockResolvedValue(() => {});
  api.watchedPending.mockResolvedValue([]);
});

const INVENTORY: FolderInventory = {
  root: '/carpeta',
  skipped: 0,
  items: [
    {
      path: '2025/emitidas.xlsx',
      size: 1,
      year: 2025,
      importable: true,
      warnings: [],
      found: {
        kind: 'libro',
        book: 'emitidas',
        invoices: 2,
        first: '2025-01-15',
        last: '2025-02-20',
        base: 150_000,
        vat: 21_000,
      },
    },
    {
      path: '2025/factura.pdf',
      size: 1,
      year: 2025,
      importable: true,
      warnings: [],
      found: {
        kind: 'factura_recibida',
        supplier: 'Proveedor SL',
        number: 'F-1',
        date: '2025-03-01',
        total: 12_100,
        in_book: false,
      },
    },
    {
      path: 'notas.pdf',
      size: 1,
      year: null,
      importable: false,
      warnings: [],
      found: { kind: 'otro', reason: 'no parece una factura ni un modelo' },
    },
  ],
};

describe('Importar', () => {
  it('avisa de los documentos nuevos de las carpetas vigiladas y lleva a revisarlos', async () => {
    api.folderGet.mockResolvedValue(null);
    api.folderScan.mockResolvedValue(INVENTORY);
    const nuevo = {
      folder: '/drive/empresa',
      path: '2026/3T/extracto.pdf',
      kind: 'Extracto bancario',
      year: 2026,
      seen: '2026-11-12',
    };
    api.watchedPending.mockResolvedValueOnce([nuevo]).mockResolvedValue([]);
    api.watchedDismiss.mockResolvedValue(undefined);
    render(<Importar onNavigate={vi.fn()} />);
    expect(await screen.findByText('Hay 1 documento nuevo en las carpetas vigiladas')).toBeTruthy();
    expect(screen.getByText('2026/3T/extracto.pdf')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Revisar esta carpeta' }));
    await waitFor(() => expect(api.folderScan).toHaveBeenCalledWith('/drive/empresa'));
    await waitFor(() =>
      expect(screen.queryByText('Hay 1 documento nuevo en las carpetas vigiladas')).toBeNull(),
    );
  });

  it('lee la carpeta, deja elegir y sólo importa lo marcado', async () => {
    api.folderGet.mockResolvedValue('/carpeta');
    api.folderScan.mockResolvedValue(INVENTORY);
    api.folderApply.mockResolvedValue([
      { path: '2025/factura.pdf', ok: true, message: 'a Recibidas para revisar (Proveedor SL)' },
    ]);
    api.receivedAiFillPending.mockResolvedValue({ filled: 1, failed: [] });
    const nav = vi.fn();
    render(<Importar onNavigate={nav} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Volver a leer' }));
    expect(await screen.findByText(/Libro de emitidas · 2 facturas/)).toBeTruthy();
    expect(screen.getByText('no parece una factura ni un modelo')).toBeTruthy();
    // Lo desconocido no se puede marcar; lo demás viene marcado.
    expect(screen.queryByLabelText('Importar notas.pdf')).toBeNull();
    fireEvent.click(screen.getByLabelText('Importar 2025/emitidas.xlsx'));
    fireEvent.click(screen.getByRole('button', { name: 'Importar 1 fichero' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, importar' }));
    await screen.findByText(/a Recibidas para revisar/);
    expect(api.folderApply).toHaveBeenCalledWith('/carpeta', ['2025/factura.pdf']);

    fireEvent.click(screen.getByRole('button', { name: 'Completar con el asistente' }));
    await screen.findByText(/ha completado 1 factura/);
    fireEvent.click(screen.getByRole('button', { name: 'Revisar 1 factura en Recibidas' }));
    expect(nav).toHaveBeenCalledWith('recibidas');
  });

  it('busca en los ficheros de la carpeta y marca de golpe los que se ven', async () => {
    api.folderGet.mockResolvedValue('/carpeta');
    api.folderScan.mockResolvedValue(INVENTORY);
    api.folderApply.mockResolvedValue([]);
    render(<Importar onNavigate={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Volver a leer' }));
    await screen.findByText(/Libro de emitidas · 2 facturas/);
    const buscar = screen.getByLabelText('Buscar en los ficheros de la carpeta');
    // Sin tildes ni mayúsculas, y por el nombre del proveedor.
    fireEvent.change(buscar, { target: { value: 'PROVEEDÓR' } });
    expect(screen.getByText(/1 fichero tiene «PROVEEDÓR»/)).toBeTruthy();
    expect(screen.queryByLabelText('Importar 2025/emitidas.xlsx')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Desmarcarlos' }));
    expect(screen.getByText('No hay nada marcado para importar.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Marcar el que se ve' }));
    fireEvent.click(screen.getByRole('button', { name: 'Importar 1 fichero' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sí, importar' }));
    await waitFor(() =>
      expect(api.folderApply).toHaveBeenCalledWith('/carpeta', ['2025/factura.pdf']),
    );
    fireEvent.change(buscar, { target: { value: 'nada así' } });
    expect(screen.getByText(/Ningún fichero tiene/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Quitar la búsqueda' }));
    expect(screen.getByLabelText('Importar 2025/emitidas.xlsx')).toBeTruthy();
  });

  it('avisa de que la carpeta está incompleta si pasa del límite o es demasiado profunda', async () => {
    api.folderGet.mockResolvedValue('/carpeta');
    api.folderScan.mockResolvedValue({ ...INVENTORY, over_limit: 2, too_deep: 1 });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Volver a leer' }));
    const alerta = await screen.findByText(/La carpeta está incompleta/);
    expect(alerta.textContent).toContain('tiene 5 ficheros y sólo se han leído los 3 primeros');
    expect(alerta.textContent).toContain('1 subcarpeta a más de ocho niveles no se mira');
  });

  it('dice cuántas lecturas de Claude Code son de copias de otro fichero', async () => {
    api.folderGet.mockResolvedValue('/carpeta');
    api.folderScan.mockResolvedValue({
      ...INVENTORY,
      claude_code: { loaded: 4, missing: [], invalid: 0, kept: 0, merged: 1, error: null },
    });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Volver a leer' }));
    const linea = await screen.findByText(/Claude Code ya ha leído 4 ficheros/);
    expect(linea.textContent).toContain(
      '1 lectura es de una copia de otro fichero con el mismo contenido: se juntan con la suya, sin perder datos.',
    );
  });

  it('compara un modelo con el de la gestoría', async () => {
    api.folderGet.mockResolvedValue(null);
    api.filedList.mockResolvedValue([
      {
        id: 7,
        model: '303',
        year: 2025,
        period: '1T',
        filename: 'x.txt',
        source: 'fichero',
        boxes: {},
      },
    ]);
    api.filedCompare.mockResolvedValue({
      filed: {
        id: 7,
        model: '303',
        year: 2025,
        period: '1T',
        filename: 'x.txt',
        source: 'fichero',
        boxes: {},
      },
      mismatches: 1,
      warnings: [],
      rows: [
        { box: 27, ours: 21_000, theirs: 25_000, diff: -4_000 },
        { box: 7, ours: 100_000, theirs: 100_000, diff: 0 },
      ],
    });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Comparar con la gestoría' }));
    fireEvent.click(await screen.findByRole('button', { name: /303/ }));
    expect(await screen.findByText(/1 casilla no cuadra/)).toBeTruthy();
    expect(screen.getByText('Total cuota devengada')).toBeTruthy();
    expect(screen.getByText(/-40,00/)).toBeTruthy();
  });

  it('compara una informativa perceptor a perceptor, con la explicación', async () => {
    api.folderGet.mockResolvedValue(null);
    api.filedList.mockResolvedValue([
      {
        id: 8,
        model: '190',
        year: 2025,
        period: '0A',
        filename: '190.txt',
        source: 'fichero',
        boxes: {},
      },
    ]);
    api.filedCompare.mockResolvedValue({
      filed: {
        id: 8,
        model: '190',
        year: 2025,
        period: '0A',
        filename: '190.txt',
        source: 'fichero',
        boxes: {},
      },
      mismatches: 2,
      warnings: [],
      rows: [
        {
          box: 2,
          ours: 100_000,
          theirs: 150_000,
          diff: -50_000,
          label: 'Percepciones íntegras',
          explanation: 'TaxPain calcula 500,00 € menos: mira el detalle por perceptor.',
        },
      ],
      lines: [
        {
          nif: '12345678Z',
          name: 'Carmen Española Española',
          key: 'G01',
          ours: null,
          theirs: 50_000,
          ours_withheld: null,
          theirs_withheld: 7_500,
          explanation: 'La gestoría lo declara y TaxPain no',
        },
      ],
    });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Comparar con la gestoría' }));
    fireEvent.click(await screen.findByRole('button', { name: /190/ }));
    expect(await screen.findByText(/2 diferencias/)).toBeTruthy();
    expect(
      screen.getByText('TaxPain calcula 500,00 € menos: mira el detalle por perceptor.'),
    ).toBeTruthy();
    expect(screen.getByText('La gestoría lo declara y TaxPain no')).toBeTruthy();
    expect(screen.getByText('Perceptor a perceptor')).toBeTruthy();
  });

  it('abre la comparación en el último año con modelos de la gestoría', async () => {
    api.folderGet.mockResolvedValue(null);
    const m = (year: number, period: string) => ({
      id: year,
      model: '303',
      year,
      period,
      filename: 'x.txt',
      source: 'fichero',
      boxes: {},
    });
    api.filedList.mockImplementation(async (y: number | null) =>
      y === null ? [m(2023, '4T'), m(2024, '2T')] : y === 2024 ? [m(2024, '2T')] : [],
    );
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Comparar con la gestoría' }));
    expect(await screen.findByText('2T')).toBeTruthy();
    expect(api.filedList).toHaveBeenCalledWith(null);
  });

  it('revisa la carpeta desde la pestaña «Revisión»', async () => {
    api.folderGet.mockResolvedValue('/carpeta');
    api.folderReviewLast.mockResolvedValue(null);
    api.folderReview.mockResolvedValue({
      generada: '2026-10-25',
      empresa: 'Ejemplo SL',
      nif: 'B12345674',
      carpeta: '/carpeta',
      resumen: {
        ficheros: 1,
        con_detalle: 1,
        sin_leer: 0,
        no_admitidos: 0,
        por_tipo: [],
        errores: 0,
        avisos: 0,
        info: 0,
        recuperable: 0,
      },
      hallazgos: [],
      tablas: [],
      pedir: [],
    });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Revisión' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Revisar la carpeta' }));
    expect(
      await screen.findByText('No se ha encontrado nada que no cuadre en la carpeta.'),
    ).toBeTruthy();
    expect(api.folderReview).toHaveBeenCalledWith('/carpeta');
  });

  it('propone los datos de la empresa de sus documentos y los aplica', async () => {
    api.folderGet.mockResolvedValue(null);
    const facts = {
      nif: 'B12345674',
      name: 'EJEMPLO, S.L.',
      street: null,
      post_code: null,
      town: null,
      constitution_date: '2019-03-12',
      capital: 300_000,
      cnae: '6201',
      iae: null,
      vat_number: null,
      administrator: null,
      notary: null,
      protocol: null,
      registry: null,
      date: '2019-03-12',
    };
    api.companyDocsList.mockResolvedValue([
      {
        id: 1,
        kind: 'escritura_constitucion',
        title: 'Escritura de constitución',
        filename: 'escritura.pdf',
        facts,
        date: '2019-03-12',
        imported_at: '',
      },
    ]);
    api.companySuggestions
      .mockResolvedValueOnce([
        {
          field: 'cnae',
          label: 'CNAE de la actividad',
          current: null,
          proposed: '6201',
          source: 'Escritura de constitución',
        },
      ])
      .mockResolvedValue([]);
    api.companyApplySuggestion.mockResolvedValue(undefined);
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Documentos y datos' }));
    expect(await screen.findByText('CNAE de la actividad')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Aplicar' }));
    await screen.findByText('CNAE de la actividad: actualizado.');
    expect(api.companyApplySuggestion).toHaveBeenCalledWith('cnae');
    fireEvent.click(screen.getByRole('button', { name: /Escritura de constitución/ }));
    expect(screen.getByText('3.000,00 €')).toBeTruthy();
  });

  it('Full IA: pide permiso al activarla y lee con IA lo que las reglas no entienden', async () => {
    api.folderGet.mockResolvedValue('/carpeta');
    api.folderScan.mockResolvedValue(INVENTORY);
    api.lecturaIaSave.mockImplementation(async (s: unknown) => s);
    api.folderAiRead.mockResolvedValue({ read: 2, reused: 0, failed: [] });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Activar Full IA…' }));
    expect(screen.getByRole('alertdialog', { name: 'Activar Full IA' }).textContent).toContain(
      'no se pueden anonimizar',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Sí, activar Full IA' }));
    await screen.findByText('activada');
    fireEvent.click(screen.getByRole('button', { name: 'Volver a leer' }));
    expect((await screen.findByText(/Leer la carpeta no envía nada/)).textContent).not.toContain(
      'Nada sale del ordenador',
    );
    fireEvent.click(await screen.findByLabelText('Importar 2025/factura.pdf'));
    fireEvent.click(await screen.findByRole('button', { name: 'Leer con IA' }));
    await screen.findByText(/Leídos con IA: 2/);
    // Sólo la nota sin reconocer (la factura recibida está completa y el libro es exacto).
    expect(api.folderAiRead).toHaveBeenCalledWith('/carpeta', ['notas.pdf'], false);
    // Lo que se había desmarcado sigue desmarcado.
    expect((screen.getByLabelText('Importar 2025/factura.pdf') as HTMLInputElement).checked).toBe(
      false,
    );
    expect((screen.getByLabelText('Importar 2025/emitidas.xlsx') as HTMLInputElement).checked).toBe(
      true,
    );
  });

  it('teclea un modelo: importes con punto decimal, casillas sin repetir', async () => {
    api.folderGet.mockResolvedValue(null);
    api.filedList.mockResolvedValue([]);
    api.filedSaveManual.mockResolvedValue(9);
    api.filedCompare.mockRejectedValue({ message: 'sin datos' });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Comparar con la gestoría' }));
    fireEvent.click(await screen.findByRole('button', { name: /Teclear las casillas/ }));
    const form = screen.getByRole('form', { name: 'Teclear un modelo del gestor' });
    fireEvent.change(screen.getByLabelText('Casilla'), { target: { value: '27' } });
    fireEvent.change(screen.getByLabelText('Importe (€)'), { target: { value: '1234.56' } });
    fireEvent.click(screen.getByRole('button', { name: 'Otra casilla' }));
    fireEvent.change(screen.getAllByLabelText('Casilla')[1] as HTMLElement, {
      target: { value: '27' },
    });
    fireEvent.change(screen.getAllByLabelText('Importe (€)')[1] as HTMLElement, {
      target: { value: '5' },
    });
    fireEvent.submit(form);
    expect((await screen.findByRole('alert')).textContent).toContain('La casilla 27 está repetida');
    expect(api.filedSaveManual).not.toHaveBeenCalled();
    fireEvent.change(screen.getAllByLabelText('Casilla')[1] as HTMLElement, {
      target: { value: '28' },
    });
    fireEvent.change(screen.getAllByLabelText('Importe (€)')[1] as HTMLElement, {
      target: { value: '12.000' },
    });
    fireEvent.submit(form);
    await vi.waitFor(() =>
      expect(api.filedSaveManual).toHaveBeenCalledWith('303', expect.any(Number), '1T', {
        27: 123456,
        28: 1200000,
      }),
    );
  });

  it('si no se pueden comparar los datos de la empresa, lo dice', async () => {
    api.folderGet.mockResolvedValue(null);
    api.companyDocsList.mockResolvedValue([]);
    api.companySuggestions.mockRejectedValue({ message: 'almacén bloqueado' });
    render(<Importar onNavigate={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Documentos y datos' }));
    expect((await screen.findByRole('alert')).textContent).toContain('almacén bloqueado');
    expect(screen.queryByText(/coinciden/)).toBeNull();
  });

  describe('Descargar de la AEAT por CSV', () => {
    const A = 'ABCD1234EFGH5678';
    const B = 'Z9Y8X7W6V5U4T3S2';
    const NEW = `Documentos AEAT/Requerimiento de información - ${A}.pdf`;
    const RESULTS: AeatCsvItem[] = [
      { csv: A, state: 'guardado', path: NEW },
      { csv: 'QWER5678TYUI9012', state: 'anulado', replacement: B },
      { csv: 'ASDF1234GHJK5678', state: 'no_encontrado' },
    ];
    const AFTER: FolderInventory = {
      ...INVENTORY,
      items: [
        ...INVENTORY.items,
        {
          path: NEW,
          size: 1,
          year: null,
          importable: false,
          warnings: [],
          found: { kind: 'otro', reason: 'no parece una factura ni un modelo' },
        },
      ],
    };
    const CERT = {
      fingerprint_sha256: 'aa11',
      subject_dn: 'CN=EMPRESA DEMO SL',
      holder_name: 'ADMINISTRADOR DEMO',
      holder_nif: '00000000T',
      entity_nif: 'B12345674',
      entity_name: 'EMPRESA DEMO SL',
    } as CertificateInfo;
    const SAME = {
      company_nif: 'B12345674',
      company_name: 'EMPRESA DEMO SL',
      cert_nif: 'B12345674',
      cert_name: 'EMPRESA DEMO SL',
      same: true,
      other_entity: false,
    };
    const deferred = <T,>() => {
      let resolve: (v: T) => void = () => {};
      let reject: (e: unknown) => void = () => {};
      const promise = new Promise<T>((ok, ko) => {
        resolve = ok;
        reject = ko;
      });
      return { promise, resolve, reject };
    };
    const paste = async (value: string) => {
      fireEvent.change(await screen.findByLabelText('Códigos CSV'), { target: { value } });
      const button = screen.getByRole('button', { name: 'Descargar' }) as HTMLButtonElement;
      await vi.waitFor(() => expect(button.disabled).toBe(false));
      return button;
    };

    it('pega los códigos, descarga, enseña los resultados y vuelve a leer la carpeta', async () => {
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      api.aeatCsvDownload.mockResolvedValue(RESULTS);
      render(<Importar onNavigate={() => {}} />);
      const box = await screen.findByLabelText('Códigos CSV');
      const button = screen.getByRole('button', { name: 'Descargar' });
      // Sin nada pegado no se puede descargar.
      expect((button as HTMLButtonElement).disabled).toBe(true);
      fireEvent.change(box, { target: { value: `CSV\n${A}\n` } });
      await vi.waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
      fireEvent.click(button);
      await screen.findByText(/1 documento guardado · 2 no se han podido descargar/);
      // Sin certificado: sólo la Sede pública.
      expect(api.aeatCsvDownload).toHaveBeenCalledWith(`CSV\n${A}\n`, null, false);
      expect(screen.getByText(/: Guardado en Documentos AEAT\/Requerimiento/)).toBeTruthy();
      expect(screen.getByText(/no tiene ningún documento con este código/)).toBeTruthy();
      // Lo descargado sale en la carpeta, con su lectura por reglas.
      expect(api.folderScan).toHaveBeenCalledWith('/carpeta');
      expect(await screen.findByText(NEW)).toBeTruthy();
      // Sin Full IA, no hay botón: se dice cómo leerlos con IA.
      expect(screen.queryByRole('button', { name: 'Leer con IA los descargados' })).toBeNull();
      expect(screen.getByText(/documento descargado que las reglas/).textContent).toContain(
        '/leer-carpeta y la ruta de esta carpeta, o activa Full IA',
      );
      // Del anulado se ofrece descargar el que lo sustituye: su línea cambia y el resto de la
      // lista (y el aviso de leer con IA) se queda.
      api.aeatCsvDownload.mockResolvedValue([{ csv: B, state: 'ya_estaba', path: 'x.pdf' }]);
      fireEvent.click(screen.getByRole('button', { name: `Descargar el que lo sustituye (${B})` }));
      await vi.waitFor(() => expect(api.aeatCsvDownload).toHaveBeenLastCalledWith(B, null, false));
      const summary = await screen.findByText(
        '1 documento guardado · 1 ya estaba · 1 no se ha podido descargar.',
      );
      expect(screen.getByText(/: Guardado en Documentos AEAT\/Requerimiento/)).toBeTruthy();
      expect(screen.getByText(/no tiene ningún documento con este código/)).toBeTruthy();
      expect(screen.queryByText(/hay otro documento que lo sustituye/)).toBeNull();
      expect(screen.getByText(/documento descargado que las reglas/)).toBeTruthy();
      // El foco no se pierde: va al resumen.
      await vi.waitFor(() => expect(document.activeElement).toBe(summary));
    });

    it('con Full IA ofrece leer con IA sólo los descargados, y usa el certificado elegido', async () => {
      api.lecturaIaGet.mockResolvedValue({ full: true, model: null, accepted_at: '2026-10-01' });
      api.listCertificates.mockResolvedValue([CERT]);
      api.aeatCheckTarget.mockResolvedValue(SAME);
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      api.aeatCsvDownload.mockResolvedValue(RESULTS);
      api.folderAiRead.mockResolvedValue({ read: 1, reused: 0, failed: [] });
      render(<Importar onNavigate={() => {}} />);
      // Opcional: de entrada, sin certificado.
      const select = (await screen.findByLabelText(
        /Certificado \(opcional\)/,
      )) as HTMLSelectElement;
      expect(select.value).toBe('');
      fireEvent.change(select, { target: { value: 'aa11' } });
      fireEvent.change(screen.getByLabelText('Códigos CSV'), { target: { value: A } });
      fireEvent.click(screen.getByRole('button', { name: 'Descargar' }));
      await vi.waitFor(() => expect(api.aeatCsvDownload).toHaveBeenCalledWith(A, 'aa11', false));
      // No se lee con IA sin pulsar el botón (envía los ficheros al proveedor).
      const read = await screen.findByRole('button', { name: 'Leer con IA los descargados' });
      expect(api.folderAiRead).not.toHaveBeenCalled();
      fireEvent.click(read);
      await screen.findByText(/Leídos con IA: 1/);
      // Sólo el nuevo que las reglas no entienden (no la nota que ya estaba en la carpeta).
      expect(api.folderAiRead).toHaveBeenCalledWith('/carpeta', [NEW], false);
    });

    it('un certificado de otra entidad no se usa; uno de otro titular, sólo confirmado', async () => {
      api.listCertificates.mockResolvedValue([CERT]);
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      api.aeatCsvDownload.mockResolvedValue(RESULTS);
      api.aeatCheckTarget.mockResolvedValue({
        ...SAME,
        cert_nif: 'B98765431',
        cert_name: 'OTRA DEMO SL',
        same: false,
        other_entity: true,
      });
      render(<Importar onNavigate={() => {}} />);
      const select = (await screen.findByLabelText(
        /Certificado \(opcional\)/,
      )) as HTMLSelectElement;
      await vi.waitFor(() => expect(select.querySelector('option[value="aa11"]')).toBeTruthy());
      fireEvent.change(select, { target: { value: 'aa11' } });
      fireEvent.click(await paste(A));
      expect((await screen.findByRole('alert')).textContent).toContain(
        'representante de B98765431',
      );
      expect(api.aeatCsvDownload).not.toHaveBeenCalled();
      // De otro titular (apoderado): se pregunta antes.
      api.aeatCheckTarget.mockResolvedValue({
        ...SAME,
        cert_nif: '00000000T',
        cert_name: 'ADMINISTRADOR DEMO',
        same: false,
      });
      fireEvent.click(screen.getByRole('button', { name: 'Descargar' }));
      const dialog = await screen.findByRole('dialog', { name: 'Descargar con este certificado' });
      expect(dialog.textContent).toContain('no de la empresa (B12345674)');
      expect(api.aeatCsvDownload).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole('button', { name: 'Descargar con este certificado' }));
      await vi.waitFor(() => expect(api.aeatCsvDownload).toHaveBeenCalledWith(A, 'aa11', true));
    });

    it('una descarga nueva que falla no deja a la vista la anterior', async () => {
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      api.aeatCsvDownload.mockResolvedValue(RESULTS);
      render(<Importar onNavigate={() => {}} />);
      fireEvent.click(await paste(A));
      await screen.findByText(/1 documento guardado/);
      api.aeatCsvDownload.mockRejectedValue({ message: 'La carpeta de la empresa ya no está' });
      fireEvent.click(await paste(B));
      expect((await screen.findByRole('alert')).textContent).toContain('ya no está');
      expect(screen.queryByText(/1 documento guardado/)).toBeNull();
      expect(screen.queryByText(/documento descargado que las reglas/)).toBeNull();
    });

    it('si se sale a mitad y se vuelve, sigue esperando y enseña el resultado', async () => {
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      const job = deferred<AeatCsvItem[]>();
      api.aeatCsvDownload.mockReturnValue(job.promise);
      const first = render(<Importar onNavigate={() => {}} />);
      fireEvent.click(await paste(A));
      await screen.findByText('Descargando de la AEAT…');
      // Mientras descarga, la carpeta no se puede cambiar ni volver a leer.
      await vi.waitFor(() =>
        expect(
          (screen.getByRole('button', { name: 'Elegir carpeta…' }) as HTMLButtonElement).disabled,
        ).toBe(true),
      );
      expect(
        (screen.getByRole('button', { name: 'Volver a leer' }) as HTMLButtonElement).disabled,
      ).toBe(true);
      first.unmount();
      render(<Importar onNavigate={() => {}} />);
      await screen.findByText('Descargando de la AEAT…');
      job.resolve(RESULTS);
      await screen.findByText(/1 documento guardado/);
      expect(api.folderScan).toHaveBeenCalledWith('/carpeta');
      await vi.waitFor(() =>
        expect(
          (screen.getByRole('button', { name: 'Elegir carpeta…' }) as HTMLButtonElement).disabled,
        ).toBe(false),
      );
    });

    it('si la descarga empezó en otra ventana, espera a que acabe y relee la carpeta', async () => {
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      api.aeatCsvRunning.mockResolvedValue(true);
      let done: () => void = () => {};
      api.onAeatCsvDone.mockImplementation(async (f: () => void) => {
        done = f;
        return () => {};
      });
      render(<Importar onNavigate={() => {}} />);
      await screen.findByText('Descargando de la AEAT…');
      await vi.waitFor(() => expect(api.onAeatCsvDone).toHaveBeenCalled());
      await act(async () => done());
      expect(screen.queryByText('Descargando de la AEAT…')).toBeNull();
      await vi.waitFor(() => expect(api.folderScan).toHaveBeenCalledWith('/carpeta'));
    });

    it('si falla la lectura con IA de alguno, se puede reintentar y el foco va al mensaje', async () => {
      api.lecturaIaGet.mockResolvedValue({ full: true, model: null, accepted_at: '2026-10-01' });
      api.folderGet.mockResolvedValue('/carpeta');
      api.folderScan.mockResolvedValue(AFTER);
      api.aeatCsvDownload.mockResolvedValue(RESULTS);
      api.folderAiRead.mockResolvedValue({ read: 0, reused: 0, failed: [[NEW, 'sin texto']] });
      render(<Importar onNavigate={() => {}} />);
      fireEvent.click(await paste(A));
      fireEvent.click(await screen.findByRole('button', { name: 'Leer con IA los descargados' }));
      const msg = await screen.findByText(/No se pudieron leer 1/);
      await vi.waitFor(() => expect(document.activeElement).toBe(msg));
      expect(screen.getByRole('button', { name: 'Leer con IA los descargados' })).toBeTruthy();
    });

    it('cambia sólo la línea del anulado por la del que lo sustituye', () => {
      const prev: AeatCsvItem[] = [
        { csv: A, state: 'guardado', path: NEW },
        { csv: 'QWER5678TYUI9012', state: 'anulado', replacement: B },
        { csv: 'ASDF1234GHJK5678', state: 'error', message: 'x' },
      ];
      const fresh: AeatCsvItem[] = [{ csv: B, state: 'guardado', path: 'b.pdf' }];
      expect(mergeCsvResults(prev, 'QWER5678TYUI9012', fresh).map((r) => r.csv)).toEqual([
        A,
        B,
        'ASDF1234GHJK5678',
      ]);
      // Sin lista anterior (se volvió a Importar), sólo lo nuevo.
      expect(mergeCsvResults(null, 'QWER5678TYUI9012', fresh)).toEqual(fresh);
    });

    it('sin carpeta elegida no deja descargar y lo dice', async () => {
      api.folderGet.mockResolvedValue(null);
      render(<Importar onNavigate={() => {}} />);
      fireEvent.change(await screen.findByLabelText('Códigos CSV'), { target: { value: A } });
      expect(
        (screen.getByRole('button', { name: 'Descargar' }) as HTMLButtonElement).disabled,
      ).toBe(true);
      expect(screen.getByText(/Elige primero la carpeta de la empresa/)).toBeTruthy();
    });

    it('si la descarga falla, lo dice', async () => {
      api.folderGet.mockResolvedValue('/carpeta');
      api.aeatCsvDownload.mockRejectedValue({
        message: 'Ya hay una comprobación o una descarga con la AEAT en marcha',
      });
      render(<Importar onNavigate={() => {}} />);
      fireEvent.change(await screen.findByLabelText('Códigos CSV'), { target: { value: A } });
      const button = screen.getByRole('button', { name: 'Descargar' });
      await vi.waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
      fireEvent.click(button);
      expect((await screen.findByRole('alert')).textContent).toContain('en marcha');
      expect(api.folderScan).not.toHaveBeenCalled();
    });

    it('explica cada resultado en lenguaje llano', () => {
      expect(csvLine({ csv: A, state: 'ya_estaba', path: 'Documentos AEAT/x.pdf' })).toBe(
        'Ya estaba en la carpeta: Documentos AEAT/x.pdf.',
      );
      expect(csvLine({ csv: A, state: 'anulado', replacement: null })).toBe(
        'La AEAT lo tiene como anulado.',
      );
      expect(csvLine({ csv: A, state: 'error', message: 'Sin conexión.' })).toBe('Sin conexión.');
    });
  });

  it('describe cada tipo de fichero', () => {
    expect(describeFound({ kind: 'banco' })).toBe('Extracto bancario');
    expect(
      describeFound({
        kind: 'cripto',
        resumen:
          'ledgers de Kraken (KRAKEN/ledgers.csv): 12 operaciones, del 2024-01-02 al 2024-06-30',
        ops: 12,
        prices: 0,
        duplicates: 0,
        first: '2024-01-02',
        last: '2024-06-30',
        unread: 2,
      }),
    ).toBe(
      'Criptomonedas · ledgers de Kraken (KRAKEN/ledgers.csv): 12 operaciones, del 2024-01-02 al 2024-06-30 · 2 ficheros del zip sin leer',
    );
    expect(
      describeFound({
        kind: 'valores',
        formato: 'XTB (PDF)',
        cuenta: '12345678',
        resumen: '3 × Compra, 3 × Venta',
        ops: 6,
        prices: 0,
        duplicates: 6,
        first: '2025-03-01',
        last: '2025-10-03',
      }),
    ).toBe('Bróker: XTB (PDF) · cuenta 12345678 · 3 × Compra, 3 × Venta · ya importado');
    expect(
      describeFound({
        kind: 'banco',
        resumen: 'BBVA …1332 EUR · 01/01/2025 a 31/03/2025 · 42 movimientos',
        last: '2025-03-31',
        cuadra: false,
      }),
    ).toBe(
      'Extracto bancario · BBVA …1332 EUR · 01/01/2025 a 31/03/2025 · 42 movimientos · no cuadra con sus saldos',
    );
    expect(
      describeFound({
        kind: 'modelo',
        model: '390',
        year: 2025,
        period: '0A',
        source: 'pdf',
        boxes: 3,
      }),
    ).toContain('3 casillas leídas del PDF');
    // El justificante de presentación de la AEAT, con o sin sus casillas.
    expect(
      describeFound({
        kind: 'modelo',
        model: '303',
        year: 2025,
        period: '4T',
        source: 'justificante',
        boxes: 12,
        boxes_read: true,
      }),
    ).toBe('Justificante de la AEAT del modelo 303 · 2025 4T · 12 casillas leídas');
    expect(
      describeFound({
        kind: 'modelo',
        model: '190',
        year: 2025,
        period: '0A',
        source: 'justificante',
        boxes: 0,
        boxes_read: false,
      }),
    ).toContain('casillas sin leer (cuenta como presentado)');
    // Sólo el justificante sin casillas leídas se propone leer con la IA.
    const item = (boxes_read: boolean): FolderItem => ({
      path: 'MODELOS/303.pdf',
      size: 1,
      year: 2025,
      importable: boxes_read,
      selectable: true,
      warnings: [],
      found: {
        kind: 'modelo',
        model: '303',
        year: 2025,
        period: '4T',
        source: 'justificante',
        boxes: boxes_read ? 12 : 0,
        boxes_read,
      },
    });
    expect(needsAi(item(true))).toBe(false);
    expect(needsAi(item(false))).toBe(true);
    // El 347 y el 349 en fichero: sus terceros, para la revisión.
    expect(
      describeFound({
        kind: 'modelo',
        model: '347',
        year: 2026,
        period: '0A',
        source: 'registros',
        boxes: 2,
      }),
    ).toContain('2 terceros leídos del fichero');
    // Lo que TaxPain ya tiene en esas fechas, para explicar por qué no se marca.
    expect(
      describeFound({
        kind: 'libro',
        book: 'emitidas',
        invoices: 2,
        first: '2025-01-15',
        last: '2025-02-20',
        base: 150_000,
        vat: 21_000,
        overlap: 1,
      }),
    ).toContain('1 factura propia en esas fechas');
    // La autorrepercusión de compras del libro de emitidas no son ventas: la base y el IVA son
    // los de lo que entra.
    const autorrepercusion = {
      kind: 'libro',
      book: 'emitidas',
      invoices: 2,
      first: '2026-01-05',
      last: '2026-01-31',
      base: 0,
      vat: 0,
      self_assessed: 2,
    } as const;
    const toda = describeFound(autorrepercusion);
    expect(toda).toContain('2 líneas · autorrepercusión de compras, no ventas');
    expect(toda).not.toContain('base');
    const parte = describeFound({
      ...autorrepercusion,
      invoices: 3,
      base: 100_000,
      vat: 21_000,
    });
    expect(parte).toContain('1 factura · base 1.000,00 € · IVA 210,00 €');
    expect(parte).toContain('y 2 líneas de autorrepercusión de compras, que no entran');
    expect(
      describeFound({
        kind: 'diario',
        entries: 2,
        first: '2025-01-02',
        last: '2025-03-31',
        already: false,
        imported: 1,
      }),
    ).toContain('1 asiento de otro lote importado en esas fechas');
  });
});
