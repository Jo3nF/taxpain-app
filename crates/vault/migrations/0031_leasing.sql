-- Contratos de arrendamiento financiero (leasing) y sus cuotas contabilizadas.
CREATE TABLE leasing (
  id          INTEGER PRIMARY KEY,
  data        TEXT NOT NULL,            -- JSON del contrato
  asset_id    INTEGER REFERENCES fixed_asset(id),
  start_entry INTEGER REFERENCES journal_entry(id),
  created_at  TEXT NOT NULL
) STRICT;

CREATE TABLE leasing_installment (
  leasing_id INTEGER NOT NULL REFERENCES leasing(id),
  n          INTEGER NOT NULL,
  entry_id   INTEGER REFERENCES journal_entry(id),
  PRIMARY KEY (leasing_id, n)
) STRICT;
