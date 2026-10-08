//! Idioma de la CLI y del visor. Los textos están en `riku/locales/<código>.yml`
//! (un archivo por idioma: `en.yml`, `es.yml`…) y se incrustan al compilar;
//! en el código se piden con [`tr!`]. El inglés es el idioma por defecto y el
//! de respaldo si falta una traducción.
//!
//! **Agregar un idioma** no toca código: copiar `en.yml` a `<código>.yml`,
//! traducir los valores y poner su nombre en `lang.name`. La lista de
//! idiomas (menú del visor, `RIKU_LANG`) sale de los archivos que haya. Ver
//! `docs/dev/development.md`, \"Translations\".
//!
//! Prioridad: `RIKU_LANG` (para scripts, CI y capturas) > la elección
//! guardada en los Ajustes del visor > inglés.

/// Texto traducido como `String`, con variables:
/// `tr!("status.loading", file = name)`.
macro_rules! tr {
    ($($arg:tt)*) => {
        rust_i18n::t!($($arg)*).into_owned()
    };
}
pub(crate) use tr;

pub const DEFAULT: &str = "en";

/// Variable de entorno que fuerza el idioma.
pub const ENV: &str = "RIKU_LANG";

/// Idiomas disponibles: código y nombre en su propio idioma (`lang.name`),
/// el inglés primero y después por código.
pub fn languages() -> Vec<(String, String)> {
    let mut codes: Vec<String> = rust_i18n::available_locales!().into_iter().map(|c| c.to_string()).collect();
    codes.sort_by_key(|c| (c != DEFAULT, c.clone()));
    codes
        .into_iter()
        .map(|c| {
            let name = rust_i18n::t!("lang.name", locale = &c).into_owned();
            (c, name)
        })
        .collect()
}

/// Código disponible para `code` (`es_PE.UTF-8` o `es-PE` cuentan como `es`).
fn supported(code: &str) -> Option<String> {
    let base = code.split(['_', '-', '.']).next().unwrap_or("").to_ascii_lowercase();
    rust_i18n::available_locales!().into_iter().map(|c| c.to_string()).find(|c| *c == base)
}

/// Idioma a usar al arrancar según `RIKU_LANG` y la preferencia guardada.
pub fn initial(saved: Option<&str>) -> String {
    std::env::var(ENV)
        .ok()
        .and_then(|v| supported(&v))
        .or_else(|| saved.and_then(supported))
        .unwrap_or_else(|| DEFAULT.to_string())
}

pub fn set(code: &str) {
    rust_i18n::set_locale(&supported(code).unwrap_or_else(|| DEFAULT.to_string()));
}

pub fn current() -> String {
    rust_i18n::locale().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn codes_are_normalized() {
        assert_eq!(supported("es_PE.UTF-8").as_deref(), Some("es"));
        assert_eq!(supported("EN-us").as_deref(), Some("en"));
        assert_eq!(supported("xx"), None);
        if std::env::var(ENV).is_err() {
            assert_eq!(initial(Some("xx")), DEFAULT);
        }
    }

    #[test]
    fn languages_come_from_the_files() {
        let langs = languages();
        assert_eq!(langs[0], ("en".to_string(), "English".to_string()));
        assert!(langs.contains(&("es".to_string(), "Español".to_string())));
    }

    #[test]
    fn english_is_the_default_and_spanish_is_available() {
        assert_eq!(rust_i18n::t!("toolbar.fit", locale = "en"), "Fit");
        assert_eq!(rust_i18n::t!("toolbar.fit", locale = "es"), "Encuadrar");
        assert_eq!(rust_i18n::t!("status.loading", locale = "es", file = "a.gds"), "Cargando a.gds …");
    }

    /// `clave: valor` de primer nivel de un `locales/*.yml` (un valor por
    /// línea, como los escribimos).
    fn read_keys(text: &str) -> BTreeMap<String, String> {
        text.lines()
            .filter(|l| !l.starts_with(['#', ' ']) && !l.starts_with("_version") && !l.trim().is_empty())
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    }

    fn placeholders(value: &str) -> BTreeSet<String> {
        value.split("%{").skip(1).filter_map(|p| p.split_once('}')).map(|(n, _)| n.to_string()).collect()
    }

    /// Cada idioma tiene exactamente las claves del inglés y las mismas
    /// variables (`%{file}`) en cada una. Si alguien agrega un texto y olvida
    /// un idioma, o traduce el nombre de una variable, este test lo dice.
    #[test]
    fn every_language_matches_english() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let en = read_keys(&std::fs::read_to_string(dir.join("en.yml")).unwrap());
        let mut problems = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yml") || path.file_stem().is_some_and(|s| s == "en") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let other = read_keys(&std::fs::read_to_string(&path).unwrap());
            for k in en.keys().filter(|k| !other.contains_key(*k)) {
                problems.push(format!("{name}: falta `{k}`"));
            }
            for k in other.keys().filter(|k| !en.contains_key(*k)) {
                problems.push(format!("{name}: `{k}` no existe en en.yml"));
            }
            for (k, v) in &other {
                if let Some(e) = en.get(k) {
                    if placeholders(e) != placeholders(v) {
                        problems.push(format!("{name}: `{k}` tiene otras variables que en.yml"));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "traducciones incompletas:\n{}", problems.join("\n"));
    }
}
