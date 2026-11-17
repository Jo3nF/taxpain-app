import { useCallback, useEffect, useState } from 'react';
import {
  type BankProposal,
  type BankRule,
  bankRuleApply,
  bankRuleDelete,
  bankRuleProposals,
  bankRules,
  errorMessage,
  euros,
  formatDate,
  plural,
} from './api';

/**
 * Movimientos con regla aprendida (texto del concepto → cuenta), para contabilizarlos juntos
 * después de revisarlos, y la lista de reglas para borrar las que no sirvan.
 */
export function BankRules({ version, onDone }: { version: number; onDone: () => void }) {
  const [props, setProps] = useState<BankProposal[]>([]);
  const [rules, setRules] = useState<BankRule[]>([]);
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    Promise.all([bankRuleProposals(), bankRules()])
      .then(([p, r]) => {
        setProps(p);
        setRules(r);
        setChosen(new Set(p.map((x) => x.tx.id)));
      })
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: recarga cuando cambian los movimientos
  useEffect(load, [load, version]);

  return (
    <div className="mt-3 text-sm">
      {props.length > 0 && (
        <section
          aria-label="Movimientos con regla"
          className="rounded-md border border-stone-200 p-3"
        >
          <p className="font-medium">
            Con regla aprendida ({props.length}): revisa y contabiliza los marcados.
          </p>
          <ul className="mt-2 space-y-1.5">
            {props.map((p) => (
              <li key={p.tx.id}>
                <label className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    checked={chosen.has(p.tx.id)}
                    onChange={(e) => {
                      const next = new Set(chosen);
                      if (e.target.checked) next.add(p.tx.id);
                      else next.delete(p.tx.id);
                      setChosen(next);
                    }}
                  />
                  {formatDate(p.tx.op_date)} · {p.tx.description} · {euros(p.tx.amount)} →{' '}
                  <span className="font-mono">{p.account}</span>
                </label>
              </li>
            ))}
          </ul>
          <button
            type="button"
            disabled={chosen.size === 0}
            className="mt-3 rounded btn-primary px-3 py-1 disabled:opacity-50"
            onClick={async () => {
              try {
                const n = await bankRuleApply([...chosen]);
                setMsg(`${plural(n, 'movimiento contabilizado', 'movimientos contabilizados')}.`);
                load();
                onDone();
              } catch (e) {
                setMsg(errorMessage(e));
              }
            }}
          >
            Contabilizar los marcados ({chosen.size})
          </button>
        </section>
      )}
      {rules.length > 0 && (
        <details className="mt-2">
          <summary>Reglas aprendidas ({rules.length})</summary>
          <ul className="mt-1 space-y-1">
            {rules.map((r) => (
              <li key={r.id} className="flex items-center gap-2">
                «{r.pattern}» ({r.sign}) → <span className="font-mono">{r.account}</span>
                <button
                  type="button"
                  className="text-red-700 underline"
                  aria-label={`Borrar la regla ${r.pattern}`}
                  onClick={() =>
                    bankRuleDelete(r.id)
                      .then(load)
                      .catch((e) => setMsg(errorMessage(e)))
                  }
                >
                  Borrar
                </button>
              </li>
            ))}
          </ul>
        </details>
      )}
      {msg && <p role="status">{msg}</p>}
    </div>
  );
}
