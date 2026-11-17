//! La red de Rampart Edge escrita a mano: un BERT pequeño (`xtremedistil-l6-h256`) con
//! embeddings de rango 48, una sola capa de codificador aplicada cuatro veces («tied layers»),
//! 8 cabezas de 32 y una cabeza que da 19 etiquetas por token.
//!
//! Sigue el grafo de `onnx/model_q4.onnx` nodo a nodo. Los pesos se descuantizan al cargar:
//! - `MatMulNBits` (4 bits sin signo, bloques de 32, punto cero 8): `[N][bloques][16 bytes]`,
//!   el nibble bajo primero;
//! - `GatherBlockQuantized` de las palabras: 4 bits **con** signo y una escala por fila;
//! - posiciones y tipo de token: 8 bits con escala y punto cero únicos (`DequantizeLinear`).
//!
//! `ner::tests::igual_que_onnxruntime` compara la salida con la de ONNX Runtime.

use std::collections::BTreeMap;

use super::onnx::Tensor;

const HIDDEN: usize = 256;
const RANK: usize = 48;
const HEADS: usize = 8;
const HEAD_DIM: usize = 32;
const FFN: usize = 1024;
const LAYERS: usize = 4;
pub(super) const LABELS: usize = 19;
pub(super) const MAX_TOKENS: usize = 384;
const EPS: f32 = 1e-12;

/// Matriz `n × k` por filas: `y[n] = x · w[n] + b[n]`.
#[derive(Debug)]
struct Linear {
    w: Vec<f32>,
    b: Vec<f32>,
    k: usize,
    n: usize,
}

impl Linear {
    fn apply(&self, x: &[f32], rows: usize, threads: usize) -> Vec<f32> {
        let (k, n) = (self.k, self.n);
        let mut y = vec![0.0; rows * n];
        // De cuatro en cuatro filas, para leer cada fila de pesos una vez para las cuatro, y los
        // bloques repartidos entre los hilos.
        par_rows(&mut y, 4 * n, threads, |blk, yb| {
            let r0 = blk * 4;
            let m = yb.len() / n;
            if m == 4 {
                let xs = [0, 1, 2, 3].map(|i| &x[(r0 + i) * k..(r0 + i + 1) * k]);
                for j in 0..n {
                    let d = dot4(xs, &self.w[j * k..(j + 1) * k]);
                    let b = self.b.get(j).copied().unwrap_or(0.0);
                    for (i, v) in d.into_iter().enumerate() {
                        yb[i * n + j] = v + b;
                    }
                }
            } else {
                for i in 0..m {
                    let xr = &x[(r0 + i) * k..(r0 + i + 1) * k];
                    for j in 0..n {
                        yb[i * n + j] = dot(xr, &self.w[j * k..(j + 1) * k])
                            + self.b.get(j).copied().unwrap_or(0.0);
                    }
                }
            }
        });
        y
    }
}

/// Cuatro productos escalares contra la misma fila de pesos.
fn dot4(xs: [&[f32]; 4], w: &[f32]) -> [f32; 4] {
    let (cw, rw) = w.as_chunks::<8>();
    let [c0, c1, c2, c3] = xs.map(|x| x.as_chunks::<8>().0);
    let mut acc = [[0.0f32; 8]; 4];
    for ((((w8, a), b), c), d) in cw.iter().zip(c0).zip(c1).zip(c2).zip(c3) {
        for l in 0..8 {
            acc[0][l] += a[l] * w8[l];
            acc[1][l] += b[l] * w8[l];
            acc[2][l] += c[l] * w8[l];
            acc[3][l] += d[l] * w8[l];
        }
    }
    let tail = cw.len() * 8;
    let mut out = [0.0; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = acc[i].iter().sum::<f32>();
        for (t, wv) in rw.iter().enumerate() {
            *o += xs[i][tail + t] * wv;
        }
    }
    out
}

/// Aplica `f(fila, salida)` a cada fila de `out` (de `width` en `width`), en hasta `threads`
/// hilos.
fn par_rows<F: Fn(usize, &mut [f32]) + Sync>(out: &mut [f32], width: usize, threads: usize, f: F) {
    let rows = out.len().div_ceil(width);
    let threads = threads.min(rows.div_ceil(4)).max(1);
    let per = rows.div_ceil(threads);
    if threads == 1 {
        for (i, row) in out.chunks_mut(width).enumerate() {
            f(i, row);
        }
        return;
    }
    std::thread::scope(|s| {
        for (t, chunk) in out.chunks_mut(per * width).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (i, row) in chunk.chunks_mut(width).enumerate() {
                    f(t * per + i, row);
                }
            });
        }
    });
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Ocho acumuladores sobre trozos de 8: el compilador lo vectoriza.
    let (ca, ra) = a.as_chunks::<8>();
    let (cb, rb) = b.as_chunks::<8>();
    let mut acc = [0.0f32; 8];
    for (x, y) in ca.iter().zip(cb) {
        for l in 0..8 {
            acc[l] += x[l] * y[l];
        }
    }
    let mut t = acc.iter().sum::<f32>();
    for (x, y) in ra.iter().zip(rb) {
        t += x * y;
    }
    t
}

#[derive(Debug)]
struct Norm {
    w: Vec<f32>,
    b: Vec<f32>,
}

impl Norm {
    fn apply(&self, x: &mut [f32]) {
        for row in x.as_chunks_mut::<HIDDEN>().0 {
            let n = HIDDEN as f32;
            let mean = row.iter().sum::<f32>() / n;
            let var = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
            let inv = 1.0 / (var + EPS).sqrt();
            for (i, v) in row.iter_mut().enumerate() {
                *v = (*v - mean) * inv * self.w[i] + self.b[i];
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct Model {
    words_q: Vec<u8>,
    words_scale: Vec<f32>,
    vocab: usize,
    proj: Linear,
    position: Vec<f32>,
    token_type: Vec<f32>,
    emb_norm: Norm,
    q: Linear,
    k: Linear,
    v: Linear,
    o: Linear,
    att_norm: Norm,
    ffn1: Linear,
    ffn2: Linear,
    out_norm: Norm,
    head: Linear,
}

struct Weights(BTreeMap<String, Tensor>);

impl Weights {
    fn get(&self, name: &str) -> Result<&Tensor, String> {
        self.0
            .get(name)
            .ok_or_else(|| format!("falta el peso {name}"))
    }

    fn floats(&self, name: &str, len: usize) -> Result<Vec<f32>, String> {
        let t = self.get(name)?;
        let v: Vec<f32> = if t.raw.is_empty() {
            t.floats.clone()
        } else {
            t.raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&c| f32::from_le_bytes(c))
                .collect()
        };
        if t.data_type != 1 || v.len() != len {
            return Err(format!("{name}: se esperaban {len} reales"));
        }
        Ok(v)
    }

    /// `MatMulNBits` de `k` entradas y `n` salidas, con su sesgo (o ninguno).
    fn nbits(&self, base: &str, k: usize, n: usize, bias: Option<&str>) -> Result<Linear, String> {
        let blocks = k.div_ceil(32);
        let q = self.get(&format!("{base}_Q4"))?;
        let scales = self.floats(&format!("{base}_scales"), n * blocks)?;
        if q.raw.len() != n * blocks * 16 {
            return Err(format!("{base}: tamaño inesperado"));
        }
        let mut w = vec![0.0; n * k];
        for j in 0..n {
            for i in 0..k {
                let (blk, pos) = (i / 32, i % 32);
                let byte = q.raw[(j * blocks + blk) * 16 + pos / 2];
                let nib = if pos % 2 == 0 { byte & 15 } else { byte >> 4 };
                w[j * k + i] = (f32::from(nib) - 8.0) * scales[j * blocks + blk];
            }
        }
        let b = match bias {
            Some(name) => self.floats(name, n)?,
            None => Vec::new(),
        };
        Ok(Linear { w, b, k, n })
    }

    /// Tabla de 8 bits con escala y punto cero únicos.
    fn dequant(&self, base: &str, len: usize) -> Result<Vec<f32>, String> {
        let q = self.get(&format!("{base}_quantized"))?;
        let scale = self.floats(&format!("{base}_scale"), 1)?[0];
        let zp = self.get(&format!("{base}_zero_point"))?;
        let zero = zp
            .raw
            .first()
            .map(|&b| i32::from(b))
            .or_else(|| zp.ints.first().copied())
            .ok_or_else(|| format!("{base}: falta el punto cero"))?;
        if q.raw.len() != len {
            return Err(format!("{base}: tamaño inesperado"));
        }
        Ok(q.raw
            .iter()
            .map(|&b| (i32::from(b) - zero) as f32 * scale)
            .collect())
    }

    fn norm(&self, base: &str) -> Result<Norm, String> {
        Ok(Norm {
            w: self.floats(&format!("{base}.weight"), HIDDEN)?,
            b: self.floats(&format!("{base}.bias"), HIDDEN)?,
        })
    }
}

impl Model {
    pub(super) fn load(onnx: &[u8], vocab: usize) -> Result<Self, String> {
        let w = Weights(super::onnx::initializers(onnx)?);
        let e = "m.encoder.embeddings.";
        let l = "m.encoder.encoder.layer.0.";
        let words = w.get(&format!("{e}word_embeddings.table.weight_Q4"))?;
        if words.data_type != 22 || words.raw.len() != vocab * RANK / 2 {
            return Err("tabla de palabras inesperada".into());
        }
        Ok(Model {
            words_q: words.raw.clone(),
            words_scale: w.floats(&format!("{e}word_embeddings.table.weight_scales"), vocab)?,
            vocab,
            proj: w.nbits("onnx::MatMul_541", RANK, HIDDEN, None)?,
            position: w.dequant(
                &format!("{e}position_embeddings.weight"),
                MAX_TOKENS * HIDDEN,
            )?,
            token_type: w.dequant(&format!("{e}token_type_embeddings.weight"), 2 * HIDDEN)?,
            emb_norm: w.norm(&format!("{e}LayerNorm"))?,
            q: w.nbits(
                "onnx::MatMul_544",
                HIDDEN,
                HIDDEN,
                Some(&format!("{l}attention.self.query.bias")),
            )?,
            k: w.nbits(
                "onnx::MatMul_551",
                HIDDEN,
                HIDDEN,
                Some(&format!("{l}attention.self.key.bias")),
            )?,
            v: w.nbits(
                "onnx::MatMul_552",
                HIDDEN,
                HIDDEN,
                Some(&format!("{l}attention.self.value.bias")),
            )?,
            o: w.nbits(
                "onnx::MatMul_558",
                HIDDEN,
                HIDDEN,
                Some(&format!("{l}attention.output.dense.bias")),
            )?,
            att_norm: w.norm(&format!("{l}attention.output.LayerNorm"))?,
            ffn1: w.nbits(
                "onnx::MatMul_559",
                HIDDEN,
                FFN,
                Some(&format!("{l}intermediate.dense.bias")),
            )?,
            ffn2: w.nbits(
                "onnx::MatMul_560",
                FFN,
                HIDDEN,
                Some(&format!("{l}output.dense.bias")),
            )?,
            out_norm: w.norm(&format!("{l}output.LayerNorm"))?,
            head: w.nbits("onnx::MatMul_612", HIDDEN, LABELS, Some("m.span_head.bias"))?,
        })
    }

    /// Fila de la tabla de palabras (4 bits con signo × escala de la fila).
    fn word(&self, id: usize) -> [f32; RANK] {
        let mut out = [0.0; RANK];
        let id = if id < self.vocab { id } else { 0 };
        let scale = self.words_scale[id];
        for (i, o) in out.iter_mut().enumerate() {
            let byte = self.words_q[(id * RANK + i) / 2];
            let nib = if i % 2 == 0 { byte & 15 } else { byte >> 4 };
            let signed = if nib > 7 {
                i32::from(nib) - 16
            } else {
                i32::from(nib)
            };
            *o = signed as f32 * scale;
        }
        out
    }

    /// Las 19 puntuaciones de cada token de una ventana (con `[CLS]` y `[SEP]`), calculadas en
    /// hasta `threads` hilos.
    pub(super) fn logits(&self, ids: &[usize], threads: usize) -> Vec<[f32; LABELS]> {
        let s = ids.len().min(MAX_TOKENS);
        let mut low = Vec::with_capacity(s * RANK);
        for &id in &ids[..s] {
            low.extend_from_slice(&self.word(id));
        }
        let mut x = self.proj.apply(&low, s, threads);
        for r in 0..s {
            for i in 0..HIDDEN {
                x[r * HIDDEN + i] += self.token_type[i] + self.position[r * HIDDEN + i];
            }
        }
        self.emb_norm.apply(&mut x);
        for _ in 0..LAYERS {
            x = self.layer(x, s, threads);
        }
        let h = self.head.apply(&x, s, threads);
        h.as_chunks::<LABELS>().0.to_vec()
    }

    fn layer(&self, x: Vec<f32>, s: usize, threads: usize) -> Vec<f32> {
        let q = self.q.apply(&x, s, threads);
        let k = self.k.apply(&x, s, threads);
        let v = self.v.apply(&x, s, threads);
        let scale = 1.0 / (HEAD_DIM as f32).sqrt();
        let mut ctx = vec![0.0; s * HIDDEN];
        par_rows(&mut ctx, HIDDEN, threads, |i, out| {
            let mut scores = vec![0.0; s];
            for h in 0..HEADS {
                let off = h * HEAD_DIM;
                let qi = &q[i * HIDDEN + off..i * HIDDEN + off + HEAD_DIM];
                let mut max = f32::NEG_INFINITY;
                for (j, sc) in scores.iter_mut().enumerate() {
                    *sc = dot(qi, &k[j * HIDDEN + off..j * HIDDEN + off + HEAD_DIM]) * scale;
                    max = max.max(*sc);
                }
                let mut sum = 0.0;
                for sc in scores.iter_mut() {
                    *sc = (*sc - max).exp();
                    sum += *sc;
                }
                let o = &mut out[off..off + HEAD_DIM];
                for (j, sc) in scores.iter().enumerate() {
                    let p = sc / sum;
                    let vj = &v[j * HIDDEN + off..j * HIDDEN + off + HEAD_DIM];
                    for (oo, vv) in o.iter_mut().zip(vj) {
                        *oo += p * vv;
                    }
                }
            }
        });
        let mut a = self.o.apply(&ctx, s, threads);
        for (ai, xi) in a.iter_mut().zip(&x) {
            *ai += xi;
        }
        self.att_norm.apply(&mut a);
        let mut f = self.ffn1.apply(&a, s, threads);
        for v in f.iter_mut() {
            *v = 0.5 * *v * (1.0 + erf(*v / std::f32::consts::SQRT_2));
        }
        let mut y = self.ffn2.apply(&f, s, threads);
        for (yi, ai) in y.iter_mut().zip(&a) {
            *yi += ai;
        }
        self.out_norm.apply(&mut y);
        y
    }
}

/// Función de error (Abramowitz y Stegun 7.1.26, error < 1,5·10⁻⁷).
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - (((((1.061_405_4 * t - 1.453_152) * t) + 1.421_413_8) * t - 0.284_496_74) * t
            + 0.254_829_6)
            * t
            * (-x * x).exp();
    if x < 0.0 { -y } else { y }
}
