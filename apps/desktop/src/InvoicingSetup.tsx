import { type FormEvent, useEffect, useId, useState } from 'react';
import {
  type CompanyInfo,
  companiesList,
  type Destination,
  detectDestinations,
  errorMessage,
  type InvoicingSettings,
  newInstallationId,
  pickFolder,
  saveInvoicingSettings,
} from './api';
import { COMPANIES_CHANGED } from './CompanySwitcher';
import { ProvinceDatalist } from './Controls';
import { provinceAfterPostCode } from './provincias';

const DEST_LABEL: Record<Destination['kind'], string> = {
  Local: 'Carpeta en este Mac',
  ICloudDrive: 'iCloud Drive',
  GoogleDrive: 'Google Drive',
  Dropbox: 'Dropbox',
};

const input = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1';
/** Cada grupo del formulario, separado del anterior por una línea, y su título. */
const group = 'border-t border-stone-200 pt-5';
const legend = 'float-left mb-3 w-full text-[0.9375rem] font-semibold text-stone-900';

function empty(installationId: string): InvoicingSettings {
  return {
    issuer: {
      id: { Nif: '' },
      name: '',
      address: { street: '', post_code: '', town: '', province: '', country: 'ES' },
      person: null,
    },
    default_series: 'A',
    rectifying_series: 'R',
    simplified_series: 'S',
    simplified_rectifying_series: 'RS',
    language: 'Es',
    default_vat: 2100,
    default_withholding: null,
    outputs: { pdf: true, facturae: false },
    destination: null,
    rounding: 'PerRate',
    // Desactivado hasta que exista el envío a la AEAT real: una factura real no debe llevar el
    // QR de pruebas ni la leyenda «Factura verificable…» (Orden HAC/1177/2024 art. 20).
    verifactu: { enabled: false, installation_id: installationId, production: false },
    footer: null,
  };
}

/** Rellena el nombre y el NIF del emisor, si están vacíos, con los de la empresa (los que se
 * pusieron al añadirla). Devuelve el mismo objeto si no hay nada que rellenar. */
export function withCompany(s: InvoicingSettings, c: CompanyInfo | undefined): InvoicingSettings {
  if (!c) return s;
  const nif = 'Nif' in s.issuer.id ? s.issuer.id.Nif.trim() : null;
  const name = s.issuer.name.trim() ? null : c.name.trim();
  const fillName = !!name && name.toLowerCase() !== 'mi empresa';
  const fillNif = nif === '' && !!c.nif?.trim();
  if (!fillName && !fillNif) return s;
  return {
    ...s,
    issuer: {
      ...s.issuer,
      name: fillName && name ? name : s.issuer.name,
      id: fillNif && c.nif ? { Nif: c.nif.trim().toUpperCase() } : s.issuer.id,
    },
  };
}

/** Ajustes de facturación: se muestran como onboarding la primera vez. */
export function InvoicingSetup({
  initial,
  onSaved,
}: {
  initial: InvoicingSettings | null;
  onSaved: (s: InvoicingSettings) => void;
}) {
  const [s, setS] = useState<InvoicingSettings | null>(initial);
  const [destinations, setDestinations] = useState<Destination[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // «Guardado» tras guardar, hasta el siguiente cambio.
  const [saved, setSaved] = useState(false);
  const provinces = useId();

  useEffect(() => {
    detectDestinations()
      .then(setDestinations)
      .catch(() => setDestinations([]));
    // Sin datos del emisor, los de la empresa activa (nombre y NIF de «Añadir empresa…»).
    const base = initial ? Promise.resolve(initial) : newInstallationId().then(empty);
    Promise.all([
      base,
      Promise.resolve()
        .then(companiesList)
        .catch(() => []),
    ])
      .then(([b, companies]) => {
        const filled = withCompany(
          b,
          Array.isArray(companies) ? companies.find((c) => c.active) : undefined,
        );
        // Sin pisar lo que ya se haya empezado a escribir.
        setS((prev) => (prev && prev !== initial ? prev : filled));
      })
      .catch((e) => setError(errorMessage(e)));
  }, [initial]);

  if (!s) return null;
  const nif = 'Nif' in s.issuer.id ? s.issuer.id.Nif : '';
  const address = s.issuer.address ?? empty('').issuer.address;
  const spain = (address?.country ?? 'ES') === 'ES';
  const person = s.issuer.person;
  const isPerson = /^[0-9XYZKLM]/i.test(nif);

  const set = (patch: Partial<InvoicingSettings>) => {
    setSaved(false);
    setS({ ...s, ...patch });
  };
  const setAddress = (patch: Partial<NonNullable<typeof address>>) =>
    set({ issuer: { ...s.issuer, address: { ...(address ?? {}), ...patch } as typeof address } });

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!s || saving) return;
    setError(null);
    setSaving(true);
    try {
      await saveInvoicingSettings(s);
      setSaved(true);
      // El nombre de la empresa puede haber cambiado (deja de ser «Mi empresa»): el selector
      // de empresas lo vuelve a leer.
      window.dispatchEvent(new Event(COMPANIES_CHANGED));
      onSaved(s);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-6 rounded-lg border border-stone-200 bg-white p-5">
      <fieldset>
        <legend className={legend}>Tus datos (aparecen en cada factura)</legend>
        <div className="form-grid clear-left">
          <label>
            NIF
            <input
              className={input}
              value={nif}
              onChange={(e) =>
                set({ issuer: { ...s.issuer, id: { Nif: e.target.value.toUpperCase().trim() } } })
              }
            />
          </label>
          <label className="span-2">
            Nombre o razón social
            <input
              className={input}
              value={s.issuer.name}
              onChange={(e) => set({ issuer: { ...s.issuer, name: e.target.value } })}
            />
          </label>
        </div>
        {isPerson && (
          <div className="form-grid mt-4">
            <label>
              Nombre
              <input
                className={input}
                value={person?.name ?? ''}
                onChange={(e) =>
                  set({
                    issuer: {
                      ...s.issuer,
                      person: {
                        name: e.target.value,
                        first_surname: person?.first_surname ?? '',
                        second_surname: person?.second_surname ?? null,
                      },
                    },
                  })
                }
              />
            </label>
            {/* Cada apellido en su campo: «De la Fuente García» no se parte por el primer espacio. */}
            <label>
              Primer apellido
              <input
                className={input}
                value={person?.first_surname ?? ''}
                onChange={(e) =>
                  set({
                    issuer: {
                      ...s.issuer,
                      person: {
                        name: person?.name ?? '',
                        first_surname: e.target.value,
                        second_surname: person?.second_surname ?? null,
                      },
                    },
                  })
                }
              />
            </label>
            <label>
              Segundo apellido
              <input
                className={input}
                value={person?.second_surname ?? ''}
                onChange={(e) =>
                  set({
                    issuer: {
                      ...s.issuer,
                      person: {
                        name: person?.name ?? '',
                        first_surname: person?.first_surname ?? '',
                        second_surname: e.target.value || null,
                      },
                    },
                  })
                }
              />
            </label>
          </div>
        )}
        <div className="form-grid mt-4">
          <label className="span-2">
            Dirección
            <input
              className={input}
              value={address?.street ?? ''}
              onChange={(e) => setAddress({ street: e.target.value })}
            />
          </label>
          <label>
            Código postal
            <input
              className={input}
              value={address?.post_code ?? ''}
              autoComplete="postal-code"
              onChange={(e) =>
                setAddress({
                  post_code: e.target.value,
                  ...(spain
                    ? {
                        province: provinceAfterPostCode(
                          address?.post_code ?? '',
                          e.target.value,
                          address?.province ?? '',
                        ),
                      }
                    : {}),
                })
              }
            />
          </label>
          <label>
            Población
            <input
              className={input}
              value={address?.town ?? ''}
              onChange={(e) => setAddress({ town: e.target.value })}
            />
          </label>
          <label>
            Provincia
            <input
              className={input}
              value={address?.province ?? ''}
              list={spain ? provinces : undefined}
              onChange={(e) => setAddress({ province: e.target.value })}
            />
            {spain && <ProvinceDatalist id={provinces} />}
          </label>
        </div>
      </fieldset>

      <fieldset className={group}>
        <legend className={legend}>Formato</legend>
        <div className="form-grid clear-left">
          <label>
            Serie de facturas
            <input
              className={input}
              value={s.default_series}
              onChange={(e) => set({ default_series: e.target.value.toUpperCase() })}
            />
          </label>
          <label>
            Serie de rectificativas
            <input
              className={input}
              value={s.rectifying_series}
              onChange={(e) => set({ rectifying_series: e.target.value.toUpperCase() })}
            />
          </label>
          <label>
            Serie de simplificadas
            <input
              className={input}
              value={s.simplified_series ?? 'S'}
              onChange={(e) => set({ simplified_series: e.target.value.toUpperCase() })}
            />
          </label>
          <label>
            Serie de rectificativas de simplificadas
            <input
              className={input}
              value={s.simplified_rectifying_series ?? 'RS'}
              onChange={(e) => set({ simplified_rectifying_series: e.target.value.toUpperCase() })}
            />
          </label>
          <label>
            Idioma del PDF
            <select
              className={input}
              value={s.language}
              onChange={(e) => set({ language: e.target.value as 'Es' | 'En' })}
            >
              <option value="Es">Español</option>
              <option value="En">Inglés</option>
            </select>
          </label>
          <label>
            IVA por defecto
            <select
              className={input}
              value={s.default_vat}
              onChange={(e) => set({ default_vat: Number(e.target.value) })}
            >
              <option value={2100}>21 %</option>
              <option value={1000}>10 %</option>
              <option value={400}>4 %</option>
            </select>
          </label>
          <label>
            Retención IRPF por defecto
            <select
              className={input}
              value={s.default_withholding ?? 0}
              onChange={(e) => set({ default_withholding: Number(e.target.value) || null })}
            >
              <option value={0}>Sin retención</option>
              <option value={1500}>15 %</option>
              <option value={700}>7 % (inicio de actividad)</option>
            </select>
          </label>
          <label className="span-2">
            Actividad (libros registro)
            <select
              className={input}
              value={`${s.activity?.code ?? 'A'}${s.activity?.kind ?? (isPerson ? '05' : '03')}`}
              onChange={(e) =>
                set({
                  activity: {
                    code: e.target.value.slice(0, 1),
                    kind: e.target.value.slice(1),
                    iae: s.activity?.iae ?? '',
                  },
                })
              }
            >
              <option value="A05">Profesional (A05)</option>
              <option value="A04">Profesional artística o deportiva (A04)</option>
              <option value="A03">Empresarial (A03)</option>
              <option value="A01">Arrendamiento de inmuebles (A01)</option>
            </select>
          </label>
          <label>
            Epígrafe IAE (4 caracteres)
            <input
              className={input}
              maxLength={4}
              value={s.activity?.iae ?? ''}
              onChange={(e) =>
                set({
                  activity: {
                    code: s.activity?.code ?? 'A',
                    kind: s.activity?.kind ?? (isPerson ? '05' : '03'),
                    iae: e.target.value.trim(),
                  },
                })
              }
            />
          </label>
          <label className="span-full">
            Pie de factura (forma de pago, IBAN…)
            <input
              className={input}
              value={s.footer ?? ''}
              onChange={(e) => set({ footer: e.target.value || null })}
            />
          </label>
          <label className="span-full">
            <input
              type="checkbox"
              checked={s.simplified_up_to_3000 ?? false}
              onChange={(e) => set({ simplified_up_to_3000: e.target.checked })}
            />{' '}
            Comercio minorista, hostelería, transporte de personas, peluquería u otra actividad del
            art. 4.2 RD 1619/2012: factura simplificada hasta 3.000 € (si no, hasta 400 €)
          </label>
        </div>
        <fieldset className="choices mt-4">
          <legend>Ficheros de cada factura</legend>
          <label>
            <input
              type="checkbox"
              checked={s.outputs.pdf}
              onChange={(e) => set({ outputs: { ...s.outputs, pdf: e.target.checked } })}
            />
            PDF
          </label>
          <label>
            <input
              type="checkbox"
              checked={s.outputs.facturae}
              onChange={(e) => set({ outputs: { ...s.outputs, facturae: e.target.checked } })}
            />
            Facturae (XML)
          </label>
          <label>
            <input
              type="checkbox"
              checked={s.outputs.ubl ?? false}
              onChange={(e) => set({ outputs: { ...s.outputs, ubl: e.target.checked } })}
            />
            UBL (B2B, EN 16931)
          </label>
        </fieldset>
      </fieldset>

      <fieldset className={group}>
        <legend className={legend}>¿Dónde guardamos una copia de cada factura?</legend>
        <div className="clear-left flex flex-wrap gap-2">
          {destinations.map((d) => (
            <button
              key={d.path}
              type="button"
              aria-pressed={s.destination?.path === d.path}
              onClick={() => set({ destination: d })}
              className="rounded-md border px-3 py-1.5 text-sm"
            >
              {DEST_LABEL[d.kind]}
            </button>
          ))}
          <button
            type="button"
            className="rounded-md border border-stone-300 px-3 py-1.5 text-sm"
            onClick={async () => {
              const p = await pickFolder();
              if (p) set({ destination: { kind: 'Local', path: p } });
            }}
          >
            Elegir otra carpeta…
          </button>
        </div>
        <p className="mt-2 text-xs text-stone-500">
          {s.destination ? s.destination.path : 'Sin copia fuera de la app'} · El original queda
          siempre cifrado en TaxPain.
        </p>
      </fieldset>

      <fieldset className={group}>
        <legend className={legend}>VERI*FACTU</legend>
        <label className="clear-left">
          <input
            type="checkbox"
            role="switch"
            aria-checked={s.verifactu.enabled}
            checked={s.verifactu.enabled}
            onChange={(e) => set({ verifactu: { ...s.verifactu, enabled: e.target.checked } })}
          />
          Generar el registro de facturación encadenado de cada factura.
        </label>
        <p className="mt-2 text-xs text-stone-500">
          TaxPain todavía no envía los registros a la AEAT real: el envío llegará en una versión
          posterior. Mientras tanto tus facturas no llevan el código QR ni la leyenda «Factura
          verificable en la sede electrónica de la AEAT», que sólo pueden ir en facturas ya
          remitidas (Orden HAC/1177/2024, art. 20). Las series de facturas, rectificativas y
          simplificadas van separadas (Reglamento de facturación, arts. 6 y 7).
        </p>
      </fieldset>

      <div className={`${group} form-actions`}>
        <button
          type="submit"
          className="rounded-md btn-primary px-4 py-2 text-sm font-medium disabled:opacity-50"
          disabled={saving}
        >
          Guardar
        </button>
        {saved && (
          <span role="status" className="text-sm text-emerald-700">
            Guardado
          </span>
        )}
      </div>
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
    </form>
  );
}
