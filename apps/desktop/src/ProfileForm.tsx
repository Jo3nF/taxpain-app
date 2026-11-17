import { type FormEvent, useEffect, useState } from 'react';
import {
  ccaaList,
  deducedProfile,
  errorMessage,
  profilePresets,
  setProfile,
  type TaxpayerProfile,
} from './api';

type Flag = {
  [K in keyof TaxpayerProfile]: TaxpayerProfile[K] extends boolean ? K : never;
}[keyof TaxpayerProfile];

const MONTHS = [
  ['January', 'enero'],
  ['February', 'febrero'],
  ['March', 'marzo'],
  ['April', 'abril'],
  ['May', 'mayo'],
  ['June', 'junio'],
  ['July', 'julio'],
  ['August', 'agosto'],
  ['September', 'septiembre'],
  ['October', 'octubre'],
  ['November', 'noviembre'],
  ['December', 'diciembre'],
] as const;
const DAYS = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/** `cierre_ejercicio` llega como `['December', 31]`. */
function closeOf(p: TaxpayerProfile): [string, number] {
  const c = p.cierre_ejercicio;
  if (Array.isArray(c) && typeof c[0] === 'string' && typeof c[1] === 'number') {
    return [c[0], c[1]];
  }
  return ['December', 31];
}

const FLAGS: { key: Flag; label: string; onlyFisica?: boolean; onlySociedad?: boolean }[] = [
  {
    key: 'retiene_trabajo_profesionales',
    label: 'Pago nóminas o facturas de profesionales con retención (111/190)',
  },
  { key: 'retiene_alquileres', label: 'Alquilo un local y practico retención (115/180)' },
  {
    key: 'retiene_capital_mobiliario',
    label: 'Pago intereses o dividendos con retención (123/193)',
  },
  {
    key: 'retiene_no_residentes',
    label: 'Pago cánones, intereses o dividendos a no residentes con retención (216/296)',
  },
  {
    key: 'obligado_347',
    label: 'Opero con algún cliente o proveedor por más de 3.005,06 € al año (347)',
  },
  {
    key: 'exento_pagos_fraccionados',
    label: 'Al menos el 70 % de mis ingresos llevan retención (sin 130)',
    onlyFisica: true,
  },
  {
    key: 'pagos_fraccionados_is',
    label: 'Hago pagos fraccionados del Impuesto sobre Sociedades (202)',
    onlySociedad: true,
  },
  {
    key: 'gran_empresa',
    label:
      'Gran empresa: más de 6.010.121,04 € de volumen de operaciones el año anterior (retenciones mensuales)',
  },
  {
    key: 'tiene_trabajadores',
    label: 'Tengo trabajadores en nómina (seguros sociales cada mes)',
  },
  {
    key: 'cotiza_reta',
    label: 'Pago la cuota de autónomos a la Seguridad Social',
    onlyFisica: true,
  },
  {
    key: 'cotiza_reta',
    label:
      'El administrador o un socio con control cotiza en el RETA como autónomo societario (cuota mensual y cambios de base)',
    onlySociedad: true,
  },
  {
    key: 'renta_fraccionada',
    label: 'Pago la Renta en dos plazos (el 40 % se paga en noviembre)',
    onlyFisica: true,
  },
  {
    key: 'operaciones_vinculadas',
    label:
      'Hago operaciones con socios, administradores o empresas del grupo por más de 250.000 € al año (232)',
    onlySociedad: true,
  },
  {
    key: 'bienes_extranjero',
    label: 'Tengo cuentas, valores o inmuebles en el extranjero por más de 50.000 € (720)',
  },
  {
    key: 'monedas_virtuales_extranjero',
    label: 'Tengo criptomonedas en plataformas del extranjero por más de 50.000 € (721)',
  },
  {
    key: 'oss',
    label: 'Vendo a consumidores de otros países de la UE con la ventanilla única (369 trimestral)',
  },
  {
    key: 'ioss',
    label:
      'Vendo envíos de fuera de la UE de hasta 150 € con la ventanilla de importación (369 mensual)',
  },
  {
    key: 'opciones_censales',
    label:
      'Avísame de los plazos para cambiar de régimen el año siguiente (censo de noviembre y diciembre)',
  },
];

export function ProfileForm({
  initial,
  onSaved,
}: {
  initial: TaxpayerProfile | null;
  onSaved: (p: TaxpayerProfile) => void;
}) {
  const [presets, setPresets] = useState<[string, TaxpayerProfile][]>([]);
  const [regions, setRegions] = useState<[string, string][]>([]);
  const [profile, setProfileState] = useState<TaxpayerProfile | null>(initial);
  const [error, setError] = useState<string | null>(null);
  const [motivos, setMotivos] = useState<string[] | null>(null);

  // Sin perfil guardado: lo que se deduce de lo presentado, para revisarlo y guardarlo.
  async function fillFromFiled() {
    setError(null);
    try {
      const d = await deducedProfile();
      if (!d) {
        setError(
          'No hay declaraciones de la empresa en TaxPain: comprueba lo presentado en la AEAT o importa los modelos de la gestoría.',
        );
        return;
      }
      setProfileState(d.perfil);
      setMotivos(d.motivos);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  useEffect(() => {
    Promise.all([profilePresets(), ccaaList()])
      .then(([p, r]) => {
        setPresets(p);
        setRegions(r);
      })
      .catch((e) => setError(errorMessage(e)));
  }, []);

  function update(patch: Partial<TaxpayerProfile>) {
    setProfileState((p) => (p ? { ...p, ...patch } : p));
  }

  // El IVA mensual obliga a llevar los libros por el SII (RIVA arts. 62.6 y 71.3) y con SII
  // no se presenta el 347 (RGAT art. 32.1.e): el perfil guardado lo refleja siempre.
  const sii = profile?.iva ? profile.iva.sii || profile.iva.periodicity === 'Mensual' : false;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!profile) return;
    const p: TaxpayerProfile = sii
      ? { ...profile, iva: { periodicity: 'Mensual', sii: true }, obligado_347: false }
      : profile;
    try {
      await setProfile(p);
      onSaved(p);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <form onSubmit={submit} className="space-y-5 rounded-lg border border-stone-200 bg-white p-4">
      <fieldset>
        <legend className="text-sm font-medium">¿Qué eres?</legend>
        <div className="mt-2 flex flex-wrap gap-2">
          {presets.map(([label, preset]) => (
            <button
              key={label}
              type="button"
              aria-pressed={profile?.persona === preset.persona}
              onClick={() => setProfileState(preset)}
              className="rounded-md border px-3 py-1.5 text-sm"
            >
              {label}
            </button>
          ))}
          {initial === null && (
            <button
              type="button"
              onClick={() => void fillFromFiled()}
              className="rounded-md border px-3 py-1.5 text-sm"
            >
              Rellenar con lo presentado
            </button>
          )}
        </div>
        {motivos && (
          <p className="mt-2 text-sm text-stone-600">
            Deducido de lo presentado: {motivos.join('; ')}. Revísalo y guárdalo para confirmarlo.
          </p>
        )}
      </fieldset>

      {profile && (
        <>
          <div className="form-grid">
            <label className="span-2">
              Comunidad autónoma de tu domicilio fiscal (sus festivos amplían los plazos)
              <select
                value={profile.ccaa ?? ''}
                onChange={(e) => update({ ccaa: e.target.value || null })}
              >
                <option value="">Sin indicar (sólo festivos nacionales)</option>
                {regions.map(([code, name]) => (
                  <option key={code} value={code}>
                    {name}
                  </option>
                ))}
              </select>
            </label>

            <label className="span-2">
              Festivos locales de tu municipio (opcional, separados por comas: 2026-06-24,
              2026-10-09)
              <input
                defaultValue={profile.festivos_locales.join(', ')}
                // Al escribir, no al salir del campo: con Intro el envío llegaba antes del blur y
                // los festivos escritos no se guardaban.
                onChange={(e) =>
                  update({
                    festivos_locales: e.target.value
                      .split(',')
                      .map((x) => x.trim())
                      .filter((x) => /^\d{4}-\d{2}-\d{2}$/.test(x)),
                  })
                }
              />
            </label>

            <label>
              Declaración de IVA
              <select
                value={profile.iva ? (sii ? 'Mensual-sii' : 'Trimestral') : 'none'}
                onChange={(e) => {
                  const v = e.target.value;
                  const mensual = v === 'Mensual-sii';
                  update({
                    iva:
                      v === 'none'
                        ? null
                        : { periodicity: mensual ? 'Mensual' : 'Trimestral', sii: mensual },
                    ...(mensual ? { obligado_347: false } : {}),
                  });
                }}
              >
                <option value="Trimestral">Trimestral</option>
                <option value="Mensual-sii">Mensual (con SII: REDEME, gran empresa, grupos)</option>
                <option value="none">No presento IVA</option>
              </select>
            </label>

            {profile.persona !== 'Fisica' && (
              <label className="span-2">
                Año desde el que pagas el IAE (cifra de negocios de 1 M€ o más dos años antes; vacío
                si estás exento)
                <input
                  type="number"
                  min={1990}
                  max={2100}
                  value={profile.iae_desde ?? ''}
                  onChange={(e) =>
                    update({ iae_desde: e.target.value ? Number(e.target.value) : null })
                  }
                  className="w-28"
                />
              </label>
            )}

            <label className="span-2">
              Operaciones con empresas de otros países de la UE (349)
              <select
                value={profile.intracomunitarias ?? 'none'}
                onChange={(e) =>
                  update({
                    intracomunitarias:
                      e.target.value === 'none'
                        ? null
                        : (e.target.value as 'Mensual' | 'Trimestral'),
                  })
                }
              >
                <option value="none">No</option>
                <option value="Trimestral">Sí, trimestral</option>
                <option value="Mensual">Sí, mensual</option>
              </select>
            </label>
          </div>

          {profile.persona === 'Juridica' &&
            (() => {
              const [month, day] = closeOf(profile);
              const max = DAYS[MONTHS.findIndex(([m]) => m === month)] ?? 31;
              return (
                <fieldset className="text-sm">
                  <legend className="font-medium">
                    Fecha de cierre del ejercicio (la de los estatutos)
                  </legend>
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <label className="field-inline">
                      Día
                      <input
                        type="number"
                        min={1}
                        max={max}
                        value={Math.min(day, max)}
                        onChange={(e) =>
                          update({
                            cierre_ejercicio: [
                              month,
                              Math.max(1, Math.min(max, Number(e.target.value) || 1)),
                            ],
                          })
                        }
                        className="w-20"
                      />
                    </label>
                    <label className="field-inline">
                      de
                      <select
                        value={month}
                        onChange={(e) => {
                          const i = MONTHS.findIndex(([m]) => m === e.target.value);
                          update({
                            cierre_ejercicio: [e.target.value, Math.min(day, DAYS[i] ?? 31)],
                          });
                        }}
                      >
                        {MONTHS.map(([m, name]) => (
                          <option key={m} value={m}>
                            {name}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                  <p className="mt-1.5 text-stone-600">
                    Lo normal es el 31 de diciembre. Si cambias la fecha, los ejercicios ya abiertos
                    conservan las suyas y el siguiente será de transición, más corto.
                  </p>
                </fieldset>
              );
            })()}

          <div className="flex flex-col items-start gap-2.5">
            {FLAGS.filter(
              (f) =>
                (!f.onlyFisica || profile.persona === 'Fisica') &&
                (!f.onlySociedad || profile.persona === 'Juridica'),
            ).map((f) => (
              <label key={f.label}>
                <input
                  type="checkbox"
                  checked={f.key === 'obligado_347' && sii ? false : profile[f.key]}
                  disabled={f.key === 'obligado_347' && sii}
                  onChange={(e) => update({ [f.key]: e.target.checked })}
                />
                {f.label}
                {f.key === 'obligado_347' && sii && (
                  <span className="text-stone-600">
                    {' '}
                    — con SII no se presenta el 347 (art. 32.1.e RGAT): la información va en los
                    libros de la Sede
                  </span>
                )}
              </label>
            ))}
          </div>

          <div className="form-actions">
            <button type="submit" className="rounded-md btn-primary px-4 py-2 text-sm font-medium">
              Guardar perfil
            </button>
          </div>
        </>
      )}
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
    </form>
  );
}
