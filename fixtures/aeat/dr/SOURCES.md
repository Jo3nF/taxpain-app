# Diseños de registro (DR) de la AEAT

Descargados el 2026-10-14 de `https://sede.agenciatributaria.gob.es/static_files/Sede/Disenyo_registro/`
(índice: Sede → Ayuda → Diseños de registro). Sin modificar. Los `.json` se generan con
`python -I tools/aeat/dr2json.py <DR> <modelo>` (openpyxl 3.1.5, xlrd 2.0.1).

| Modelo | Fichero | Ruta | SHA-256 | Vigencia según el índice |
|---|---|---|---|---|
| 303 | `DR303e26v101.xlsx` | `DR_300_399/archivos_26/` | `0be8b156da2250c6b11f6253e0165221ed2e549ec4c65a562021bec6b9b8489b` | Ejercicio 2026 y siguientes (28/01/26) |
| 130 | `DR130e15v12.xls` | `DR_100_199/archivos/` | `58f731b0c72eff7fd23484000c74e73e0ac803a5167065176d78cac8712f5fe7` | Orden HAP/258/2015, 2019 y siguientes |
| 111 | `dr111e16v18.xls` | `DR_100_199/archivos/` | `5f65e85078360f576b3a80a8b4dda54eaa752b410f1e2ea00b52ec5a84d5edff` | Orden EHA/3127/2009, 2019 y siguientes (mayo 2021) |
| 115 | `DR115e15v13.xls` | `DR_100_199/archivos/` | `d0b4c2cb344fe2d4b91a082bc85cc7906e383be792134504ddaedd0dc8bb2694` | Orden EHA/3435/2007, 2019 y siguientes |
| 349 | `DR_Anexo_349.pdf` | `DR_300_399/archivos_20/` | `874db49c9aff4d9c024bdee52f869123a9815c09272a0066cf81421ace1a8335` | Orden HAC/174/2020, 2020 y siguientes |

`349.json` está **transcrito a mano** del PDF (registros de 500 posiciones: declarante, operador
y rectificación); un test comprueba que los campos son contiguos y suman 500.
| 390 | `dr390e2026.xlsx` | `DR_300_399/archivos_26/` | `5ce047995347c2b5ad6e5d4e407f126eeb9bd214602ed7c99e9f85175176ec15` | Ejercicio 2026 y siguientes (v1.00; Orden HAC/27/2026). Descargado el 2026-10-28; `390.json` con `tools/aeat/dr2json.py` |
| 200 | `DR200e25.xls` (v1.02, no se guarda: 11 MB) | `DR_200_299/archivos_25/` | `92392cdb46d8e7c7f6e4e6477306570e15edfd64d5ea3e6d631e5cf847dd5509` | Ejercicio 2025. `200.json` con `tools/aeat/dr2json.py`; `200-cuentas.json` con `tools/aeat/m200_cuentas.py` |
| 202 | `DR202e25.xlsx` | `DR_200_299/archivos_25/` | `04e7b349b24b982d985195d4ae38b68e72e75d606cf66c7ef30b62b281c7f82c` | Ejercicio 2025 y siguientes |
| 190 | `DISENOS_LOGICOS_190_2025.pdf` | `DR_100_199/archivos_25/` | `a7d1092f78620431812354e560a5146a3ae244e0aed69d9d58c353370ba0134d` | Orden EHA/3127/2009 (act. Orden HAC/1431/2025) |
| 180 | `DR_Mod_180_2023.pdf` | `DR_100_199/archivos_23/` | `f4f4a0e9c8150288489e0a3058bedbd9a3b9f58bdba5e58b1599b7c7f5fc6cda` | Orden HAP/1732/2014 (act. Orden HFP/1284/2023) |
| 347 | `347.pdf` | `DR_300_399/archivos/` | `14583ea5ee04441356c4572d30630d46bfe0818366e575ac186370dd3fe38361` | Ejercicio 2025 y siguientes |
| 184 | `DR_Modelo_184_2025.pdf` | `DR_100_199/` | `b1e2b7884946061d5584f03253745b2936bcd99359f4b096acfe238c42bc9fba` | Orden HAP/2250/2015 (act. Orden HAC/1430/2025), ejercicio 2025. Descargado el 2026-10-23 |

| 123 | `DR123e24.xls` (versión 2.0) | `DR_100_199/archivos_24/` | `85ffe058c1728a50d11d3c6fcfe03f77e172e0458b53920a620aa434d66d07b4` | Orden EHA/3435/2007, ejercicio 2024 y siguientes. Descargado el 2026-10-24; `123.json` con `tools/aeat/dr2json.py` |
| 193 | `DR_Modelo_193_2025.pdf` | `DR_100_199/` | `25ab19076ecc1116c660f8d4b0c47523e6cd271d89869895699c91843f562489` | Orden EHA/3377/2011 (act. Orden HAC/1430/2025), ejercicio 2025. Descargado el 2026-10-24 |

`190.json`, `180.json`, `347.json`, `184.json` y `193.json` también están transcritos a mano de
sus PDF; sus reglas y dudas están en `190-notas.md`, `180-notas.md`, `347-notas.md`,
`184-notas.md` y `193-notas.md`.

El DR del 200 (`DR_200_299/archivos_25/DR200e25.xls`, 11 MB, SHA-256
`92392cdb46d8e7c7f6e4e6477306570e15edfd64d5ea3e6d631e5cf847dd5509`, 77 páginas) **no se
incluye** en el repositorio por tamaño: sólo se usaron sus números de casilla para el borrador
de la liquidación (`crates/models/src/m200.rs`).

## Diseños de ejercicios anteriores (`anteriores/`)

Para leer los ficheros de declaraciones ya presentadas de otros años (`taxpain_models::dr_versions`).
Descargados el 2026-10-28 de las páginas «Diseños de registro → ejercicios anteriores» de la Sede
(`https://sede.agenciatributaria.gob.es/Sede/ayuda/disenos-registro/ejercicios-anteriores-modelos-100-199.html`,
`-200-299.html` y `-300-399.html`), sin modificar; el `.json` de cada uno con `tools/aeat/dr2json.py`.
El periodo va entre paréntesis: trimestre o pago fraccionado; las anuales cuentan como el 4.
Avisos de los originales: la página 03 del 303 de 2017 a 2020 tiene un hueco de 4 posiciones
entre las casillas [65] y [66], y la marca de cierre de la página 7 del 390 de 2017 a 2023 está
mal escrita («</T3900700>»): TaxPain escribe y busca la marca a partir de la de inicio.

| Modelo | Fichero | Carpeta de la Sede | SHA-256 | Periodos |
|---|---|---|---|---|
| 123 | `DR123e15v12.xls` | `DR_100_199/` | `cc24b134697856e14a34b03b85eaeb1195e21e740cfc4d0a6f735a4dd8d6ea70` | 2015 (1) a 2018 (4) |
| 123 | `DR123e15v13.xls` | `DR_100_199/` | `21ec4feed2950c57c689a772166952b3c2245e7101bce7836c2baedc1f4f8dbd` | 2019 (1) a 2023 (4) |
| 202 | `DR202e17v47.xlsx` | `DR_200_299/` | `c5bef798fc270bf6360e33a6ef1f89ea841669462cd8956c4c0edb6983cb76e1` | 2017 (1) a 2018 (1) |
| 202 | `DR202e17v48.xlsx` | `DR_200_299/` | `7828b3ce0bb64da4a5b2f0fef866e3145be9059f53a682eb89c7475996e73a2a` | 2018 (2) a 2018 (3) |
| 202 | `DR202e23.xlsx` | `DR_200_299/` | `1e4881439e25417df5a8584bffd7149ca0952e2df53963b19c0346572259bec7` | 2023 (1) a 2024 (3) |
| 202 | `DR202v52.xlsx` | `DR_200_299/` | `96160cf2a82a4e6f2c9c9848c6061b2cfe5c4877de7a455126704af86f3ac7db` | 2019 (1) a 2022 (3) |
| 303 | `DR303e17v20_04.xlsx` | `DR_300_399/` | `85775f8df5f7ba17f60b565eae41e710ec423285164867b0de83799fc6f07795` | 2017 (1) a 2017 (4) |
| 303 | `DR303e18v10_10.xlsx` | `DR_300_399/` | `3fbad46fdad001a6faab1a68cd7e13958901e208cc7ab888786fc7ba44065098` | 2018 (1) a 2018 (3) |
| 303 | `DR303e18v10_40.xlsx` | `DR_300_399/` | `4683a167c73019f935e6e78a6c2ef1d76f71a1c78207fdf8bc0098703892db25` | 2018 (4) a 2018 (4) |
| 303 | `DR303e20v10_20.xlsx` | `DR_300_399/` | `bd570be2599aa7aba4740282fc06dd573e8d0d03dd0717ae3c1d26e0fae0446d` | 2019 (1) a 2020 (4) |
| 303 | `DR303e21v103.xlsx` | `DR_300_399/` | `cb0226f444e4412336b155655d5b2afda0b60e7e87d6dcda6eafd6497e5f4ee7` | 2021 (1) a 2021 (2) |
| 303 | `DR303e21v200.xlsx` | `DR_300_399/` | `185b8612c9cf4a22b4ed919db0ba45d21b03b7168da957fca3b5758f567ce249` | 2021 (3) a 2021 (4) |
| 303 | `DR303e22.xlsx` | `DR_300_399/` | `6648f6b319579e49cd5bfdaae69e7451db75767e7f19da0b90383b25b79b3f60` | 2022 (1) a 2022 (4) |
| 303 | `DR303e23.xlsx` | `DR_300_399/` | `72e463cb29984f535c9f56917d788ff0641965f116aeab47da5f76a59eecfbe4` | 2023 (1) a 2023 (4) |
| 303 | `DR303e24.xlsx` | `DR_300_399/` | `8b1f74b58b9293e60f9ea6fa3cc352a35ca3fe7d09a6705f122585e7f7da65b9` | 2024 (1) a 2024 (2) |
| 303 | `DR303e24v200.xlsx` | `DR_300_399/` | `2095dd633413f4aed28053bc88402461d80865f454156c01ebc4a2ab68cb76a8` | 2024 (3) a 2024 (4) |
| 303 | `DR303e25.xlsx` | `DR_300_399/` | `6c3d7eeb714e0deb52f91d7e8dbadeb83f16c1d32d25f9e871756f3ddf0117e6` | 2025 (1) a 2025 (4) |
| 390 | `dr390e2017v3.xlsx` | `DR_300_399/` | `e94e3a5471cde24105197e13362a4e3d12284ba4aeb3585bfd812c85dc0bd4cd` | 2017 (4) a 2017 (4) |
| 390 | `dr390e2018v101.xlsx` | `DR_300_399/` | `6ad240de8090017dca775ed4234a90a43b163e3bb44e042502b5c74a9a9e2d61` | 2018 (4) a 2018 (4) |
| 390 | `dr390e2019v101.xlsx` | `DR_300_399/` | `7d4caff7bbdd668fa2ddd03f8751632394ed4010a81301b5fc787272f2371bc1` | 2019 (4) a 2020 (4) |
| 390 | `dr390e2021.xlsx` | `DR_300_399/` | `0164fbea6f500a63950b762f5b5e43c5d771f84ac8d260e70dc1497acaed4246` | 2021 (4) a 2021 (4) |
| 390 | `dr390e2022.xlsx` | `DR_300_399/` | `7c6554f3182df51daaec37284dd891eb925e1f92df7e69bc01b8ccfb8e4f26fe` | 2022 (4) a 2022 (4) |
| 390 | `dr390e2023v100.xlsx` | `DR_300_399/` | `179c02eddc8bab411c249fc3fda19c7015d668e1dd7930d4af79f38998b9c5a7` | 2023 (4) a 2023 (4) |
| 390 | `dr390e2024.xlsx` | `DR_300_399/` | `8be79bacc86034c3c7951d2ea671c030800ed9a4cc3f52b9e5d407bc19bc03f0` | 2024 (4) a 2024 (4) |
| 390 | `dr390e2025.xlsx` (v1.02, actualizado 05/12/2025) | `DR_300_399/archivos_25/` | `6d33d8a4245976e55dc31ff85065b420f76d1588110dc1eb541a8039c5e3f252` | 2025 (4) a 2025 (4). El generador del 390 escribe con él 2025 y los ejercicios anteriores (`m390::dr_json`) |

El 200 cambia cada año y sus originales pesan de 8 a 11 MB: no se guardan. Su JSON es compacto (`python -I tools/aeat/dr2json.py <DR> 200 --compacto`: marcas de página, campos de importe con casilla y último campo de cada página). Comprobado el 2026-10-28 con declaraciones reales de 2018 a 2025: cada página mide lo que dice su diseño y el balance cuadra.

| Modelo | Fichero | Carpeta de la Sede | SHA-256 | Periodos |
|---|---|---|---|---|
| 200 | `DR200-2017-1_00.xls` (no se guarda: 8 MB; JSON compacto) | `ant_200_299/archivos` | `4e11acb94c3cae59248c954860e2c35cf33dfd93976f9494344c3136b610da4e` | 2017 (4) a 2017 (4) |
| 200 | `DR200-2018.xls` (no se guarda: 9 MB; JSON compacto) | `ant_200_299/archivos` | `c561bc77e6525adac94bb5f5c8e69974fc0e20151506706b1376725effdd3625` | 2018 (4) a 2018 (4) |
| 200 | `DR200-2019.xls` (no se guarda: 9 MB; JSON compacto) | `ant_200_299/archivos` | `ce43a011a3cd0774aed54cb08379724f7077fd423ed94026dc58a590798d4797` | 2019 (4) a 2019 (4) |
| 200 | `DR200-2020.xls` (no se guarda: 10 MB; JSON compacto) | `DR_200_299/archivos_20` | `1754e834411536c05ed25ebd4e0395e1330806c91c69154614c6178acf0e80de` | 2020 (4) a 2020 (4) |
| 200 | `DR200-2021.xls` (no se guarda: 10 MB; JSON compacto) | `DR_200_299/archivos_21` | `3db076b4c6b604cce9dc2f1550a1b8ce792c48981149bf2e9f732970555a2620` | 2021 (4) a 2021 (4) |
| 200 | `DR200-2022.xls` (no se guarda: 10 MB; JSON compacto) | `DR_200_299/archivos_22` | `a69d73692c40f8be07005e2df8add251f9949a2d6780f3816189372bdd98f69d` | 2022 (4) a 2022 (4) |
| 200 | `DR200-2023.xls` (no se guarda: 10 MB; JSON compacto) | `DR_200_299/archivos_23` | `07ab6b44ac6b46317d965ef59680aeb0a3380f197f55f94af824383d228190b6` | 2023 (4) a 2023 (4) |
| 200 | `DR200-2024.xls` (no se guarda: 11 MB; JSON compacto) | `DR_200_299/archivos_24` | `ed4df89a451abc2184bc60a1d13ff53a3d38e9a6201698fb635cf0b8ee455218` | 2024 (4) a 2024 (4) |
