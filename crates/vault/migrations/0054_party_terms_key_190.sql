-- Clave y subclave del 190 de un proveedor profesional, fijada en sus condiciones: G.01 (15 %,
-- general), G.02 (7 %, recaudadores, mediadores y delegados de Loterías, art. 95.1 RIRPF 2.º
-- párrafo), G.03 (7 %, inicio de actividad, art. 95.1 3.º párrafo), G.04 (7 %, rendimientos del
-- año anterior < 15.000 €, art. 101.5.a LIRPF); H.01 a H.04 (actividades agrícolas, ganaderas,
-- forestales y en estimación objetiva, art. 95.4 a 95.6 RIRPF). NULL: G.01, como hasta ahora.
ALTER TABLE party_terms ADD COLUMN key_190 TEXT
    CHECK (key_190 IS NULL OR key_190 IN ('G01','G02','G03','G04','H01','H02','H03','H04'));
