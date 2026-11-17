# Rampart Edge (detector local de datos personales)

Modelo de National Design Studio que marca nombres, direcciones y otros datos personales en un
texto. TaxPain sólo usa sus etiquetas de nombre (`NAME`) y dirección (`ADDRESS`); el resto lo
cubren las reglas con dígito de control de `crates/assistant/src/pii.rs`.

- Origen: https://huggingface.co/nationaldesignstudio/rampart-edge, revisión
  `69a99a3f0d3cbdb8db11f2ac97899f654be109bf`.
- Licencia: CC0 1.0 (`LICENSE`, copiada tal cual).
- Ficheros y SHA-256 (los comprueba `ner::tests::modelo_con_la_huella_publicada`):
  - `model_q4.onnx` (`onnx/model_q4.onnx` en el origen):
    `73fce00c229d8318d7e1f8decccc5ad2dbf7aa45a872364b54d632c38a9eb16c`
  - `vocab.txt`: `ee0684683c7fb15744d0136e671cd7def76ae4d4ddafb72ced864e94c8d32c9a`
  - `config.json`: `01e20d1a4acb7af422d9182c8d3ff2d506c6cd6b0ca26bead3697f5e380ae00a`
- Va dentro del binario (`include_bytes!`): no se descarga nada al usar la app.
- La red está escrita a mano en `crates/assistant/src/ner/bert.rs` (sin ONNX Runtime). Para
  cambiar de versión: sustituir los ficheros, actualizar las huellas y regenerar
  `crates/assistant/src/ner/golden.json` con ONNX Runtime y numpy (tokens con el
  `tokenizer.json` oficial).
