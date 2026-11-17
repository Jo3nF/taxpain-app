import { useCallback, useEffect, useState } from 'react';
import { type ChartAccount, chart, pgcAccounts } from './api';

/** Nombre de una cuenta: el del plan de la empresa o el de la cuenta del PGC más larga que la
 * contiene (mismo criterio que `taxpain_app::accounting::account_name`). */
export function nameFor(code: string, plan: Map<string, string>, pgc: Map<string, string>) {
  const own = plan.get(code);
  if (own) return own;
  for (let n = Math.min(4, code.length); n >= 3; n--) {
    const name = pgc.get(code.slice(0, n));
    if (name) return name;
  }
  return undefined;
}

/** Plan de cuentas de la empresa y cuadro del PGC para nombrar y buscar cuentas. */
export function useAccounts(year: number) {
  const [plan, setPlan] = useState<ChartAccount[]>([]);
  const [pgc, setPgc] = useState<[string, string][]>([]);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    chart(year)
      .then(setPlan)
      .catch((e) => setError(String(e?.message ?? e)));
  }, [year]);
  useEffect(() => {
    reload();
    pgcAccounts()
      .then(setPgc)
      .catch(() => setPgc([]));
  }, [reload]);
  const planNames = new Map(plan.filter((a) => a.name).map((a) => [a.code, a.name]));
  const pgcNames = new Map(pgc);
  return {
    plan,
    pgc,
    error,
    reload,
    name: (code: string) => nameFor(code, planNames, pgcNames),
    /** ¿Existe ya (en el plan o en el cuadro)? Si no, se crea al registrar el asiento. */
    known: (code: string) => planNames.has(code) || pgcNames.has(code),
  };
}
