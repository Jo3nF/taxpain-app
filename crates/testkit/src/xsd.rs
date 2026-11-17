//! Validación XML contra XSD para tests, mediante libxml2 del sistema (siempre presente en
//! macOS; en Linux basta `libxml2.so.2`, sin cabeceras). Nunca accede a la red.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[repr(C)]
struct XmlError {
    domain: c_int,
    code: c_int,
    message: *const c_char,
    level: c_int,
    file: *const c_char,
    line: c_int,
}

type StructuredErrorFn = unsafe extern "C" fn(*mut c_void, *const XmlError);

#[cfg_attr(
    target_os = "linux",
    link(name = "libxml2.so.2", kind = "dylib", modifiers = "+verbatim")
)]
#[cfg_attr(not(target_os = "linux"), link(name = "xml2"))]
unsafe extern "C" {
    fn xmlSchemaNewParserCtxt(url: *const c_char) -> *mut c_void;
    fn xmlSchemaSetParserStructuredErrors(
        ctxt: *mut c_void,
        f: StructuredErrorFn,
        ctx: *mut c_void,
    );
    fn xmlSchemaParse(ctxt: *mut c_void) -> *mut c_void;
    fn xmlSchemaFreeParserCtxt(ctxt: *mut c_void);
    fn xmlSchemaNewValidCtxt(schema: *mut c_void) -> *mut c_void;
    fn xmlSchemaSetValidStructuredErrors(ctxt: *mut c_void, f: StructuredErrorFn, ctx: *mut c_void);
    fn xmlSchemaValidateDoc(ctxt: *mut c_void, doc: *mut c_void) -> c_int;
    fn xmlSchemaFreeValidCtxt(ctxt: *mut c_void);
    fn xmlSchemaFree(schema: *mut c_void);
    fn xmlReadMemory(
        buf: *const c_char,
        size: c_int,
        url: *const c_char,
        encoding: *const c_char,
        options: c_int,
    ) -> *mut c_void;
    fn xmlFreeDoc(doc: *mut c_void);
}

/// XML_PARSE_NONET: prohibido acceder a la red.
const XML_PARSE_NONET: c_int = 1 << 11;

unsafe extern "C" fn collect(ctx: *mut c_void, err: *const XmlError) {
    // SAFETY: `ctx` es el `Vec<String>` que pasamos nosotros; `err` lo da libxml2.
    unsafe {
        let errors = &mut *(ctx as *mut Vec<String>);
        if !err.is_null() && !(*err).message.is_null() {
            let msg = CStr::from_ptr((*err).message)
                .to_string_lossy()
                .trim()
                .to_owned();
            errors.push(format!("línea {}: {msg}", (*err).line));
        }
    }
}

/// libxml2 no es reentrante en la carga de esquemas: serializamos.
static LOCK: Mutex<()> = Mutex::new(());

/// Copia `xsd_dir` a un temporal reescribiendo `schemaLocation` remotos a ficheros locales.
pub fn local_schema_dir(xsd_dir: &Path, rewrites: &[(&str, &str)]) -> tempfile_dir::Dir {
    let dir = tempfile_dir::Dir::new();
    for entry in std::fs::read_dir(xsd_dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "xsd") {
            let mut s = std::fs::read_to_string(&p).unwrap();
            for (from, to) in rewrites {
                s = s.replace(from, to);
            }
            std::fs::write(dir.path().join(p.file_name().unwrap()), s).unwrap();
        }
    }
    dir
}

/// Valida `xml` contra el esquema `xsd`. `Err` con los mensajes de libxml2.
pub fn validate(xsd: &Path, xml: &str) -> Result<(), Vec<String>> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut errors: Vec<String> = Vec::new();
    let ctx = (&mut errors as *mut Vec<String>).cast::<c_void>();
    let url = CString::new(xsd.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: llamadas a libxml2 con punteros válidos durante toda la función; se liberan
    // todos los objetos creados.
    unsafe {
        let pctx = xmlSchemaNewParserCtxt(url.as_ptr());
        if pctx.is_null() {
            return Err(vec!["no se pudo abrir el XSD".into()]);
        }
        xmlSchemaSetParserStructuredErrors(pctx, collect, ctx);
        let schema = xmlSchemaParse(pctx);
        xmlSchemaFreeParserCtxt(pctx);
        if schema.is_null() {
            errors.insert(0, "XSD no válido".into());
            return Err(errors);
        }
        let doc = xmlReadMemory(
            xml.as_ptr().cast(),
            c_int::try_from(xml.len()).unwrap(),
            c"doc.xml".as_ptr(),
            std::ptr::null(),
            XML_PARSE_NONET,
        );
        if doc.is_null() {
            xmlSchemaFree(schema);
            return Err(vec!["XML mal formado".into()]);
        }
        let vctx = xmlSchemaNewValidCtxt(schema);
        xmlSchemaSetValidStructuredErrors(vctx, collect, ctx);
        let rc = xmlSchemaValidateDoc(vctx, doc);
        xmlSchemaFreeValidCtxt(vctx);
        xmlFreeDoc(doc);
        xmlSchemaFree(schema);
        if rc == 0 { Ok(()) } else { Err(errors) }
    }
}

/// Directorio temporal mínimo (evita depender de `tempfile` en el testkit).
pub mod tempfile_dir {
    use super::PathBuf;

    #[derive(Debug)]
    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new() -> Self {
            // Contador del proceso: en macOS el reloj tiene resolución de microsegundos y dos
            // tests en paralelo podían obtener el mismo nombre (y uno borraba el del otro).
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            loop {
                let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let p = std::env::temp_dir()
                    .join(format!("taxpain-xsd-{}-{nanos}-{n}", std::process::id()));
                // `create_dir` (no `_all`) falla si ya existe: así nunca se comparte.
                if std::fs::create_dir(&p).is_ok() {
                    return Dir(p);
                }
            }
        }

        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Default for Dir {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XSD: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="a"><xs:complexType><xs:sequence>
        <xs:element name="b" type="xs:integer"/>
      </xs:sequence></xs:complexType></xs:element></xs:schema>"#;

    #[test]
    fn valida_y_rechaza() {
        let dir = tempfile_dir::Dir::new();
        let xsd = dir.path().join("t.xsd");
        std::fs::write(&xsd, XSD).unwrap();
        assert_eq!(validate(&xsd, "<a><b>1</b></a>"), Ok(()));
        let err = validate(&xsd, "<a><b>x</b></a>").unwrap_err();
        assert!(err.iter().any(|e| e.contains("integer")), "{err:?}");
        assert!(validate(&xsd, "<a><c/></a>").is_err());
        assert!(validate(&xsd, "<a>").is_err());
    }
}
