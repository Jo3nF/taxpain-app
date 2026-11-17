import { useEffect, useState } from 'react';
import {
  type CompanyOverview,
  companiesOverview,
  companySwitch,
  type DeadlineStatus,
  errorMessage,
  formatDate,
} from './api';
import { COMPANIES_CHANGED } from './CompanySwitcher';
import { Consolidation } from './Consolidation';

const label = (d: DeadlineStatus) => d.label;

/** De cuándo es lo presentado que se tiene en cuenta en «Vencido» para una empresa. */
function checkNote(c: CompanyOverview): string {
  return c.status.aeat_checked_at
    ? `Comprobado con la AEAT el ${formatDate(c.status.aeat_checked_at.slice(0, 10))}`
    : 'Sin comprobar con la AEAT';
}

/**
 * Todas las empresas del Mac de un vistazo: lo vencido sin presentar y lo que vence en las
 * próximas semanas. Cuenta como presentado lo presentado desde TaxPain y lo que tiene la AEAT
 * según la última «Comprobar lo presentado» (también lo de la gestoría). Sólo aparece con más
 * de una empresa. `onCheck` lleva a Modelos para comprobarlo.
 */
export function CompaniesOverview({ onCheck }: { onCheck?: () => void }) {
  const [list, setList] = useState<CompanyOverview[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    const load = () =>
      companiesOverview()
        .then(setList)
        .catch((e) => setMsg(errorMessage(e)));
    load();
    // Al modificar o borrar una empresa desde el selector.
    window.addEventListener(COMPANIES_CHANGED, load);
    return () => window.removeEventListener(COMPANIES_CHANGED, load);
  }, []);
  if (list.length < 2) return msg ? <p role="status">{msg}</p> : null;
  return (
    <section aria-label="Todas las empresas" className="mt-6 text-sm">
      <h3 className="text-base font-semibold">Todas las empresas</h3>
      <table className="mt-2 w-full">
        <thead>
          <tr className="text-left text-stone-600">
            <th>Empresa</th>
            <th>Vencido sin presentar</th>
            <th>Próximos 45 días</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {list.map((c) => (
            <tr key={c.id} className="border-t border-stone-200 align-top">
              <td>
                {c.name}
                {c.nif && <span className="block text-xs text-stone-500">{c.nif}</span>}
                <span className="block text-xs text-stone-500">{checkNote(c)}</span>
              </td>
              <td className={c.status.overdue.length ? 'text-red-700' : 'text-stone-500'}>
                {!c.status.has_profile
                  ? 'Sin perfil fiscal'
                  : c.status.overdue.length === 0
                    ? '—'
                    : c.status.overdue.map((d) => (
                        <span key={label(d)} className="block">
                          {label(d)} (venció el {formatDate(d.due_on)})
                        </span>
                      ))}
              </td>
              <td>
                {c.status.upcoming.length === 0
                  ? '—'
                  : c.status.upcoming.map((d) => (
                      <span key={label(d)} className="block">
                        {label(d)} · {formatDate(d.due_on)}
                        {d.filed ? ' · presentado' : ''}
                      </span>
                    ))}
              </td>
              <td className="text-right">
                {c.active ? (
                  <span className="text-stone-500">abierta</span>
                ) : (
                  <button
                    type="button"
                    className="text-link underline"
                    onClick={() =>
                      companySwitch(c.id)
                        .then(() => window.location.reload())
                        .catch((e) => setMsg(errorMessage(e)))
                    }
                  >
                    Abrir
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="mt-1 text-stone-500">
        «Vencido» cuenta como presentado lo presentado desde TaxPain y lo que tiene la AEAT según la
        última comprobación de cada empresa (también lo que presentó la gestoría).
      </p>
      {list.some((c) => !c.status.aeat_checked_at) && (
        <p className="mt-1 text-amber-800">
          {list
            .filter((c) => !c.status.aeat_checked_at)
            .map((c) => c.name)
            .join(', ')}{' '}
          {list.filter((c) => !c.status.aeat_checked_at).length === 1
            ? 'no se ha comprobado'
            : 'no se han comprobado'}{' '}
          con la AEAT: lo que presentó la gestoría puede salir como vencido. Ábrela y, en Modelos,
          pulsa «Comprobar lo presentado».
          {onCheck && list.some((c) => c.active && !c.status.aeat_checked_at) && (
            <>
              {' '}
              <button type="button" className="text-link underline" onClick={onCheck}>
                Comprobar la empresa abierta
              </button>
            </>
          )}
        </p>
      )}
      {msg && <p role="status">{msg}</p>}
      <Consolidation companies={list} />
    </section>
  );
}
