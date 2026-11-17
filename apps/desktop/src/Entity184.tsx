import { useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  type Partner,
  partnerDelete,
  partnerSave,
  partnersList,
  type Settings184,
  saveSettings184,
  settings184,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** «33,3333» → diezmilésimas de punto (333333); inválido → null. */
export function parsePct4(s: string): number | null {
  const n = Number(s.replace(',', '.'));
  if (!(n > 0 && n <= 100)) return null;
  return Math.round(n * 10000);
}

const showPct = (p: number) => (p / 10000).toLocaleString('es-ES', { maximumFractionDigits: 4 });

/**
 * Datos del 184 de una entidad en atribución de rentas (comunidad de bienes, sociedad civil):
 * socios o comuneros con su participación y datos de la entidad que no salen de la
 * contabilidad.
 */
export function Entity184({ onChange }: { onChange: () => void }) {
  const [partners, setPartners] = useState<Partner[]>([]);
  const [s, setS] = useState<Settings184 | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [f, setF] = useState({ nif: '', name: '', province: '', address: '', pct: '', joined: '' });
  const load = useCallback(() => {
    partnersList()
      .then(setPartners)
      .catch((e) => setMsg(errorMessage(e)));
    settings184()
      .then(setS)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const total = partners.filter((p) => !p.left_on).reduce((a, p) => a + p.pct, 0);
  const done = (m: string) => {
    setMsg(m);
    load();
    onChange();
  };
  const num = (k: keyof Settings184) => (e: { target: { value: string } }) =>
    s && setS({ ...s, [k]: Number(e.target.value) });
  const text = (k: keyof Settings184) => (e: { target: { value: string } }) =>
    s && setS({ ...s, [k]: e.target.value });

  return (
    <details className="mt-4 rounded-lg border border-stone-200 bg-white p-3 text-sm" open>
      <summary className="font-medium">Socios y datos de la entidad (184)</summary>
      <section aria-label="Socios o comuneros" className="mt-2">
        <ul>
          {partners.map((p) => (
            <li key={p.id} className="flex flex-wrap items-center gap-3">
              <span className="font-mono">{p.nif}</span>
              <span>{p.name}</span>
              <span>{showPct(p.pct)} %</span>
              {p.joined_on && <span>alta {p.joined_on}</span>}
              {p.left_on && <span>baja {p.left_on}</span>}
              <ConfirmAction
                label="Quitar"
                question={`¿Quitar a ${p.name} de los socios?`}
                confirmLabel="Sí, quitar"
                onConfirm={() =>
                  partnerDelete(p.id)
                    .then(() => done(`${p.name} quitado.`))
                    .catch((e) => setMsg(errorMessage(e)))
                }
              />
            </li>
          ))}
        </ul>
        {partners.length > 0 && total !== 1_000_000 && (
          <p className="text-amber-800">
            Las participaciones de los socios actuales suman {showPct(total)} % (deben sumar 100 %).
          </p>
        )}
        <div className="form-grid mt-3">
          <label>
            NIF
            <input value={f.nif} onChange={(e) => setF({ ...f, nif: e.target.value })} />
          </label>
          <label className="span-2">
            Nombre (apellidos y nombre)
            <input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} />
          </label>
          <label>
            Provincia (01-52)
            <input
              inputMode="numeric"
              value={f.province}
              onChange={(e) => setF({ ...f, province: e.target.value })}
            />
          </label>
          <label>
            Participación %
            <input value={f.pct} onChange={(e) => setF({ ...f, pct: e.target.value })} />
          </label>
          <label className="span-2">
            Domicilio
            <input value={f.address} onChange={(e) => setF({ ...f, address: e.target.value })} />
          </label>
          <label>
            Alta (si fue este año)
            <input
              type="date"
              value={f.joined}
              onChange={(e) => setF({ ...f, joined: e.target.value })}
            />
          </label>
          <div className="form-actions">
            <button
              type="button"
              className="rounded border border-stone-300 px-3 py-1"
              onClick={() => {
                const pct = parsePct4(f.pct);
                if (pct === null) {
                  setMsg('Revisa la participación (de 0,0001 a 100).');
                  return;
                }
                partnerSave({
                  id: 0,
                  nif: f.nif,
                  name: f.name,
                  province: f.province.padStart(2, '0'),
                  address: f.address,
                  pct,
                  joined_on: f.joined || null,
                  left_on: null,
                })
                  .then(() => {
                    setF({ nif: '', name: '', province: '', address: '', pct: '', joined: '' });
                    done('Socio guardado.');
                  })
                  .catch((e) => setMsg(errorMessage(e)));
              }}
            >
              Añadir socio
            </button>
          </div>
        </div>
      </section>
      {s && (
        <section
          aria-label="Datos de la entidad"
          className="form-grid mt-5 border-t border-stone-200 pt-4"
        >
          <label>
            Entidad
            <select value={s.entity_type} onChange={num('entity_type')}>
              <option value={2}>Comunidad de bienes</option>
              <option value={1}>Sociedad civil</option>
              <option value={3}>Herencia yacente</option>
              <option value={5}>Otra</option>
            </select>
          </label>
          <label>
            Actividad
            <select value={s.main_activity} onChange={num('main_activity')}>
              <option value={1}>Empresarial</option>
              <option value={2}>Profesional</option>
              <option value={3}>Alquiler de inmuebles</option>
            </select>
          </label>
          {s.main_activity !== 3 ? (
            <>
              <label>
                Estimación directa
                <select value={s.regime} onChange={num('regime')}>
                  <option value={2}>Simplificada</option>
                  <option value={1}>Normal</option>
                </select>
              </label>
              <label className="span-2">
                Tipo
                <select value={s.activity_kind} onChange={num('activity_kind')}>
                  <option value={1}>Empresarial mercantil</option>
                  <option value={2}>Agrícola o ganadera</option>
                  <option value={3}>Otra empresarial</option>
                  <option value={4}>Profesional artística o deportiva</option>
                  <option value={5}>Otra profesional</option>
                </select>
              </label>
              <label>
                Epígrafe IAE
                <input value={s.iae} onChange={text('iae')} />
              </label>
              <label className="flex gap-2">
                <input
                  type="checkbox"
                  checked={s.cash}
                  onChange={(e) => setS({ ...s, cash: e.target.checked })}
                />
                Criterio de cobros y pagos
              </label>
            </>
          ) : (
            <>
              <label>
                Días alquilado
                <input inputMode="numeric" value={s.rent_days} onChange={num('rent_days')} />
              </label>
              <label className="span-2">
                Referencia catastral
                <input value={s.cadastral_ref} onChange={text('cadastral_ref')} />
              </label>
              <label className="span-2">
                Situación
                <select value={s.rent_situation} onChange={num('rent_situation')}>
                  <option value={1}>España (salvo País Vasco y Navarra)</option>
                  <option value={2}>País Vasco</option>
                  <option value={3}>Navarra</option>
                  <option value={4}>Sin referencia catastral</option>
                </select>
              </label>
            </>
          )}
          <label>
            NIF del representante
            <input value={s.representative_nif} onChange={text('representative_nif')} />
          </label>
          <label className="span-2">
            Representante
            <input value={s.representative_name} onChange={text('representative_name')} />
          </label>
          <div className="span-full form-actions">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1"
              onClick={() =>
                saveSettings184(s)
                  .then(() => done('Datos de la entidad guardados.'))
                  .catch((e) => setMsg(errorMessage(e)))
              }
            >
              Guardar datos
            </button>
          </div>
        </section>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
    </details>
  );
}
