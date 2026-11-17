//! La ventana principal sólo enseña la propia app: una navegación a otra página (un
//! `location.href = 'https://…'` de un script que no debería estar ahí) se para aquí. Los
//! enlaces de verdad (la Sede, el BOE) se abren en el navegador con el plugin `opener`.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, Url};

pub fn navegacion<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("navegacion")
        .on_navigation(|webview, url| {
            let dev = if cfg!(debug_assertions) {
                webview.app_handle().config().build.dev_url.clone()
            } else {
                None
            };
            let ok = permitida(url, dev.as_ref());
            if !ok {
                // Sin la dirección: podría llevar datos.
                tracing::warn!(
                    esquema = url.scheme(),
                    "navegación fuera de la app bloqueada"
                );
            }
            ok
        })
        .build()
}

/// El origen propio de la app (`tauri://localhost` en el Mac, `http://tauri.localhost` en
/// otras plataformas) y, en desarrollo, el servidor de Vite (`devUrl`).
fn permitida(url: &Url, dev: Option<&Url>) -> bool {
    match (url.scheme(), url.host_str()) {
        ("tauri", Some("localhost")) => true,
        ("http" | "https", Some("tauri.localhost")) => url.port().is_none(),
        ("about", _) => url.as_str() == "about:blank",
        _ => dev.is_some_and(|d| d.origin() == url.origin()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn solo_el_origen_de_la_app() {
        let dev = u("http://localhost:5173");
        for ok in [
            "tauri://localhost",
            "tauri://localhost/index.html?demo=1#x",
            "http://tauri.localhost/",
            "about:blank",
        ] {
            assert!(permitida(&u(ok), None), "{ok}");
        }
        assert!(permitida(&u("http://localhost:5173/?demo=1"), Some(&dev)));
        for no in [
            "https://example.com/?d=datos",
            "http://localhost:5173/",
            "tauri://localhost.example.com/",
            "http://tauri.localhost.example.com/",
            "http://tauri.localhost:8080/",
            "file:///Users/ana/Documents/a.html",
            "data:text/html,hola",
            "about:srcdoc",
        ] {
            assert!(!permitida(&u(no), None), "{no}");
        }
        // Con devUrl, sólo ese origen (no otro puerto).
        assert!(!permitida(&u("http://localhost:5174/"), Some(&dev)));
    }
}
