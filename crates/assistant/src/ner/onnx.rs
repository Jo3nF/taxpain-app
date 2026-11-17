//! Lector mínimo de un fichero ONNX: sólo los pesos (`graph.initializer`) por su nombre.
//!
//! El grafo no se interpreta: la arquitectura está escrita a mano en `bert.rs`. Aquí basta con
//! leer los campos de `TensorProto` que usa el modelo (dimensiones, tipo, nombre, `raw_data` y,
//! en los escalares, `float_data`/`int32_data`), con el formato de protobuf.

use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(super) struct Tensor {
    pub dims: Vec<usize>,
    pub data_type: i32,
    pub raw: Vec<u8>,
    pub floats: Vec<f32>,
    pub ints: Vec<i32>,
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.buf.get(self.pos).ok_or("varint cortado")?;
            self.pos += 1;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("varint demasiado largo".into())
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let len = usize::try_from(self.varint()?).map_err(|_| "longitud no válida")?;
        let end = self.pos.checked_add(len).ok_or("longitud no válida")?;
        let s = self.buf.get(self.pos..end).ok_or("campo cortado")?;
        self.pos = end;
        Ok(s)
    }

    /// Siguiente campo: (número, tipo de cable).
    fn key(&mut self) -> Result<(u64, u8), String> {
        let k = self.varint()?;
        Ok((k >> 3, (k & 7) as u8))
    }

    fn skip(&mut self, wire: u8) -> Result<(), String> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.pos += 8,
            2 => {
                self.bytes()?;
            }
            5 => self.pos += 4,
            _ => return Err(format!("tipo de campo {wire} no admitido")),
        }
        if self.pos > self.buf.len() {
            return Err("campo cortado".into());
        }
        Ok(())
    }
}

fn tensor(buf: &[u8]) -> Result<(String, Tensor), String> {
    let mut r = Reader::new(buf);
    let mut t = Tensor::default();
    let mut name = String::new();
    while !r.done() {
        match r.key()? {
            (1, 0) => t
                .dims
                .push(usize::try_from(r.varint()?).map_err(|_| "dimensión")?),
            (1, 2) => {
                let mut p = Reader::new(r.bytes()?);
                while !p.done() {
                    t.dims
                        .push(usize::try_from(p.varint()?).map_err(|_| "dimensión")?);
                }
            }
            (2, 0) => t.data_type = r.varint()? as i32,
            (4, 2) => {
                for &c in r.bytes()?.as_chunks::<4>().0 {
                    t.floats.push(f32::from_le_bytes(c));
                }
            }
            (4, 5) => {
                let c = r.buf.get(r.pos..r.pos + 4).ok_or("float cortado")?;
                t.floats.push(f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
                r.pos += 4;
            }
            (5, 0) => t.ints.push(r.varint()? as i32),
            (5, 2) => {
                let mut p = Reader::new(r.bytes()?);
                while !p.done() {
                    t.ints.push(p.varint()? as i32);
                }
            }
            (8, 2) => name = String::from_utf8_lossy(r.bytes()?).into_owned(),
            (9, 2) => t.raw = r.bytes()?.to_vec(),
            (_, w) => r.skip(w)?,
        }
    }
    Ok((name, t))
}

/// Los pesos del modelo, por nombre.
pub(super) fn initializers(model: &[u8]) -> Result<BTreeMap<String, Tensor>, String> {
    let mut out = BTreeMap::new();
    let mut r = Reader::new(model);
    while !r.done() {
        match r.key()? {
            // ModelProto.graph
            (7, 2) => {
                let mut g = Reader::new(r.bytes()?);
                while !g.done() {
                    match g.key()? {
                        // GraphProto.initializer
                        (5, 2) => {
                            let (name, t) = tensor(g.bytes()?)?;
                            out.insert(name, t);
                        }
                        (_, w) => g.skip(w)?,
                    }
                }
            }
            (_, w) => r.skip(w)?,
        }
    }
    if out.is_empty() {
        return Err("el modelo no tiene pesos".into());
    }
    Ok(out)
}
