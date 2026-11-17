//! Reconocimiento de texto en el Mac con Vision (`VNRecognizeTextRequest`): fotos y PDF
//! escaneados se leen aquí, sin enviar nada fuera. Las páginas de un PDF se dibujan con PDFKit
//! y se pasan a Vision como imagen.

#![allow(unsafe_code)]
// llamadas Objective-C a Vision, PDFKit y AppKit
// Coordenadas y tamaños de imagen (no hay importes): reales.
#![allow(clippy::float_arithmetic)]

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSData, NSDictionary, NSSize, NSString};
use objc2_pdf_kit::{PDFDisplayBox, PDFDocument};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
};

/// Como mucho se leen tantas páginas de un PDF escaneado.
pub const MAX_PAGES: usize = 10;

/// Como mucho tantos píxeles en una imagen que se lee (Vision la descomprime entera: 4 bytes por
/// píxel). Algo más de los 48 Mpx de las fotos grandes de un iPhone.
pub const MAX_PIXELS: u64 = 50_000_000;

/// Lado máximo, en píxeles, de una página de PDF dibujada para leerla.
const MAX_SIDE: f64 = 3000.0;

/// Texto de una imagen (PNG, JPEG, HEIC, TIFF, WebP…), de arriba abajo. El tamaño se mira en la
/// cabecera antes de dársela a Vision: una imagen de pocos KB puede decir que mide 100.000 ×
/// 100.000 píxeles.
pub fn image_text(bytes: &[u8]) -> Result<String, String> {
    check_image(bytes)?;
    let data = NSData::with_bytes(bytes);
    recognize(&data)
}

/// ¿Se puede dar a Vision? Sólo formatos que se reconocen y de hasta [`MAX_PIXELS`].
fn check_image(bytes: &[u8]) -> Result<(), String> {
    let (w, h) = dimensions(bytes).ok_or_else(|| "formato de imagen no reconocido".to_owned())?;
    if w.saturating_mul(h) > MAX_PIXELS {
        return Err(format!(
            "la imagen es demasiado grande para leerla ({w} × {h} píxeles)"
        ));
    }
    Ok(())
}

fn be16(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u16::from_be_bytes(
        b.get(i..i + 2)?.try_into().ok()?,
    )))
}

fn be32(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u32::from_be_bytes(
        b.get(i..i + 4)?.try_into().ok()?,
    )))
}

fn le16(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u16::from_le_bytes(
        b.get(i..i + 2)?.try_into().ok()?,
    )))
}

fn le32(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u32::from_le_bytes(
        b.get(i..i + 4)?.try_into().ok()?,
    )))
}

/// Ancho y alto (en píxeles) que declara la cabecera de una imagen; de las que llevan varias
/// (miniaturas, páginas de un TIFF, teselas de un HEIC), el mayor.
fn dimensions(b: &[u8]) -> Option<(u64, u64)> {
    let mayor = |a: Option<(u64, u64)>, n: (u64, u64)| match a {
        Some(a) if a.0.saturating_mul(a.1) >= n.0.saturating_mul(n.1) => Some(a),
        _ => Some(n),
    };
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return (b.get(12..16)? == b"IHDR").then_some((be32(b, 16)?, be32(b, 20)?));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        // Segmentos hasta el comienzo de los datos: el tamaño va en los SOFn.
        let mut i = 2;
        let mut out = None;
        while i + 4 <= b.len() {
            if b[i] != 0xFF {
                return out;
            }
            let m = b[i + 1];
            if m == 0xFF {
                i += 1;
                continue;
            }
            if m == 0xD9 || m == 0xDA {
                return out;
            }
            if (0xD0..=0xD7).contains(&m) || m == 0x01 {
                i += 2;
                continue;
            }
            let len = usize::try_from(be16(b, i + 2)?).ok()?;
            if (0xC0..=0xCF).contains(&m) && ![0xC4, 0xC8, 0xCC].contains(&m) {
                out = mayor(out, (be16(b, i + 7)?, be16(b, i + 5)?));
            }
            i += 2 + len;
        }
        return out;
    }
    if b.starts_with(b"GIF8") {
        return Some((le16(b, 6)?, le16(b, 8)?));
    }
    if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        return tiff(b);
    }
    if b.starts_with(b"RIFF") && b.get(8..12)? == b"WEBP" {
        return match b.get(12..16)? {
            b"VP8 " => Some((le16(b, 26)? & 0x3FFF, le16(b, 28)? & 0x3FFF)),
            b"VP8L" => {
                let v = le32(b, 21)?;
                Some((1 + (v & 0x3FFF), 1 + ((v >> 14) & 0x3FFF)))
            }
            b"VP8X" => {
                let w = le32(b, 24)? & 0x00FF_FFFF;
                let h = le32(b, 27)? & 0x00FF_FFFF;
                Some((w + 1, h + 1))
            }
            _ => None,
        };
    }
    if b.get(4..8)? == b"ftyp" {
        // HEIC, HEIF, AVIF: las cajas `ispe` (de la imagen, de la cuadrícula y de cada tesela).
        let mut out = None;
        let mut i = 0;
        while let Some(p) = b.get(i..)?.windows(4).position(|w| w == b"ispe") {
            let at = i + p;
            if let (Some(w), Some(h)) = (be32(b, at + 8), be32(b, at + 12)) {
                out = mayor(out, (w, h));
            }
            i = at + 4;
        }
        return out;
    }
    None
}

/// Tamaño de un TIFF: el mayor de sus imágenes (IFD).
fn tiff(b: &[u8]) -> Option<(u64, u64)> {
    let le = b[0] == b'I';
    let u16_ = |i: usize| if le { le16(b, i) } else { be16(b, i) };
    let u32_ = |i: usize| if le { le32(b, i) } else { be32(b, i) };
    let mut ifd = usize::try_from(u32_(4)?).ok()?;
    let mut out: Option<(u64, u64)> = None;
    for _ in 0..64 {
        if ifd == 0 {
            break;
        }
        let n = usize::try_from(u16_(ifd)?).ok()?;
        let (mut w, mut h) = (None, None);
        for k in 0..n {
            let e = ifd + 2 + k * 12;
            let tag = u16_(e)?;
            let valor = match u16_(e + 2)? {
                3 => u16_(e + 8)?,
                4 => u32_(e + 8)?,
                _ => continue,
            };
            match tag {
                256 => w = Some(valor),
                257 => h = Some(valor),
                _ => {}
            }
        }
        if let (Some(w), Some(h)) = (w, h)
            && out.is_none_or(|o| o.0.saturating_mul(o.1) < w.saturating_mul(h))
        {
            out = Some((w, h));
        }
        ifd = usize::try_from(u32_(ifd + 2 + n * 12)?).ok()?;
    }
    out
}

/// Escala para dibujar una página de PDF de `w` × `h` puntos: unos 200 ppp, sin pasar de
/// [`MAX_SIDE`] píxeles por ningún lado (una página de 200.000 × 1.000 puntos no se dibuja a
/// 555.000 píxeles de ancho).
fn page_scale(w: f64, h: f64) -> f64 {
    (200.0 / 72.0_f64)
        .min(MAX_SIDE / h.max(1.0))
        .min(MAX_SIDE / w.max(1.0))
}

fn recognize(data: &NSData) -> Result<String, String> {
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        data,
        &NSDictionary::new(),
    );
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    let langs = NSArray::from_retained_slice(&[
        NSString::from_str("es-ES"),
        NSString::from_str("ca-ES"),
        NSString::from_str("en-US"),
    ]);
    request.setRecognitionLanguages(&langs);
    let as_request: Retained<VNRequest> =
        Retained::into_super(Retained::into_super(request.clone()));
    let requests = NSArray::from_retained_slice(&[as_request]);
    handler
        .performRequests_error(&requests)
        .map_err(|e| e.localizedDescription().to_string())?;
    let Some(results) = request.results() else {
        return Ok(String::new());
    };
    // Vision da cada línea con su caja (origen abajo a la izquierda): de arriba abajo y de
    // izquierda a derecha.
    let mut lines: Vec<(f64, f64, String)> = Vec::new();
    for obs in results.iter() {
        // SAFETY: boundingBox no tiene efectos y la observación es válida.
        let b = unsafe { obs.boundingBox() };
        if let Some(c) = obs.topCandidates(1).firstObject() {
            lines.push((
                b.origin.y + b.size.height,
                b.origin.x,
                c.string().to_string(),
            ));
        }
    }
    lines.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    Ok(lines
        .into_iter()
        .map(|l| l.2)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Texto de un PDF escaneado: cada página (hasta [`MAX_PAGES`]) se dibuja a unos 200 ppp y se
/// lee con Vision.
pub fn pdf_text(bytes: &[u8]) -> Result<String, String> {
    let data = NSData::with_bytes(bytes);
    // SAFETY: PDFDocument se crea y usa en este hilo con datos propios.
    let doc = unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }
        .ok_or_else(|| "no es un PDF válido".to_owned())?;
    let pages = unsafe { doc.pageCount() }.min(MAX_PAGES);
    let mut out = Vec::new();
    for i in 0..pages {
        // SAFETY: índice dentro de pageCount.
        let Some(page) = (unsafe { doc.pageAtIndex(i) }) else {
            continue;
        };
        let bounds = unsafe { page.boundsForBox(PDFDisplayBox::MediaBox) };
        // 72 ppp → ~200 ppp, sin pasar de 3000 px de alto ni de ancho.
        let scale = page_scale(bounds.size.width, bounds.size.height);
        let size = NSSize::new(bounds.size.width * scale, bounds.size.height * scale);
        let image = unsafe { page.thumbnailOfSize_forBox(size, PDFDisplayBox::MediaBox) };
        let Some(tiff) = image.TIFFRepresentation() else {
            continue;
        };
        out.push(recognize(&tiff)?);
    }
    Ok(out.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 2, 0, 0, 0]);
        b
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
        b.extend([0xFF, 0xC0, 0, 11, 8]);
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend([1, 1, 0x11, 0]);
        b.extend([0xFF, 0xDA, 0, 2]);
        b
    }

    #[test]
    fn tamano_de_las_imagenes() {
        assert_eq!(dimensions(&png(4032, 3024)), Some((4032, 3024)));
        assert_eq!(dimensions(&jpeg(1200, 1600)), Some((1200, 1600)));
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend(2u16.to_le_bytes());
        for (tag, v) in [(256u16, 3000u32), (257, 2000)] {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(4u16.to_le_bytes());
            tiff.extend(1u32.to_le_bytes());
            tiff.extend(v.to_le_bytes());
        }
        tiff.extend(0u32.to_le_bytes());
        assert_eq!(dimensions(&tiff), Some((3000, 2000)));
        let mut heic = b"\0\0\0\x18ftypheic".to_vec();
        heic.extend(b"....ispe\0\0\0\0");
        heic.extend(8064u32.to_be_bytes());
        heic.extend(6048u32.to_be_bytes());
        assert_eq!(dimensions(&heic), Some((8064, 6048)));
        assert_eq!(dimensions(b"no es una imagen"), None);
    }

    #[test]
    fn una_imagen_enorme_no_llega_a_vision() {
        // Una foto de 48 Mpx, sí; una cabecera de 100.000 × 100.000, no (sin descomprimirla).
        assert!(check_image(&png(8064, 6048)).is_ok());
        let e = image_text(&png(100_000, 100_000)).unwrap_err();
        assert!(e.contains("demasiado grande"), "{e}");
        assert!(image_text(b"texto suelto").is_err());
    }

    #[test]
    fn las_paginas_se_dibujan_sin_pasar_de_3000_px_por_ningun_lado() {
        let s = page_scale(595.0, 842.0);
        assert!((s - 200.0 / 72.0).abs() < 1e-9 || 842.0 * s <= 3000.0);
        for (w, h) in [
            (200_000.0, 1000.0),
            (1000.0, 200_000.0),
            (14_400.0, 14_400.0),
        ] {
            let s = page_scale(w, h);
            assert!(
                w * s <= 3000.0 + 1e-6 && h * s <= 3000.0 + 1e-6,
                "{w} × {h}: {s}"
            );
        }
    }
}
