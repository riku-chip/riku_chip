//! Idioma de la CLI y del visor. Los textos están en `riku/locales/*.yml`
//! (inglés y español por clave: `cli.yml`, `gui.yml`) y se incrustan al
//! compilar; en el código se piden con [`tr!`]. El inglés es el idioma por
//! defecto y el de respaldo si falta una traducción.
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

/// Idiomas disponibles: código y nombre (en su propio idioma, para el menú).
pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("es", "Español")];

pub const DEFAULT: &str = "en";

/// Variable de entorno que fuerza el idioma.
pub const ENV: &str = "RIKU_LANG";

fn supported(code: &str) -> Option<&'static str> {
    // `es_PE.UTF-8` o `es-PE` cuentan como `es`.
    let base = code.split(['_', '-', '.']).next().unwrap_or("").to_ascii_lowercase();
    LANGUAGES.iter().map(|(c, _)| *c).find(|c| *c == base)
}

/// Idioma a usar al arrancar según `RIKU_LANG` y la preferencia guardada.
pub fn initial(saved: Option<&str>) -> &'static str {
    std::env::var(ENV)
        .ok()
        .and_then(|v| supported(&v))
        .or_else(|| saved.and_then(supported))
        .unwrap_or(DEFAULT)
}

pub fn set(code: &str) {
    rust_i18n::set_locale(supported(code).unwrap_or(DEFAULT));
}

pub fn current() -> String {
    rust_i18n::locale().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_normalized() {
        assert_eq!(supported("es_PE.UTF-8"), Some("es"));
        assert_eq!(supported("EN-us"), Some("en"));
        assert_eq!(supported("fr"), None);
        assert_eq!(initial(Some("fr")), if std::env::var(ENV).is_ok() { initial(None) } else { DEFAULT });
    }

    /// Cada clave de `gui.yml` debe tener los dos idiomas: si alguien agrega
    /// un texto y olvida uno, la GUI mostraría el de respaldo sin avisar.
    #[test]
    fn every_key_has_both_languages() {
        for (name, text) in [("gui.yml", include_str!("../locales/gui.yml")), ("cli.yml", include_str!("../locales/cli.yml"))] {
            check_file(name, text);
        }
    }

    fn check_file(name: &str, text: &str) {
        let mut current: Option<(String, Vec<String>)> = None;
        let mut problems = Vec::new();
        let mut check = |entry: Option<(String, Vec<String>)>| {
            if let Some((key, langs)) = entry {
                for (code, _) in LANGUAGES {
                    if !langs.iter().any(|l| l == code) {
                        problems.push(format!("{name}: {key}: falta `{code}`"));
                    }
                }
            }
        };
        for line in text.lines() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') || line.starts_with("_version") {
                continue;
            }
            if !line.starts_with(' ') {
                check(current.take());
                current = Some((line.trim_end_matches(':').to_string(), Vec::new()));
            } else if let Some((_, langs)) = current.as_mut() {
                if let Some((code, _)) = line.trim().split_once(':') {
                    langs.push(code.to_string());
                }
            }
        }
        check(current.take());
        assert!(problems.is_empty(), "traducciones incompletas:\n{}", problems.join("\n"));
    }

    #[test]
    fn english_is_the_default_and_spanish_is_available() {
        assert_eq!(rust_i18n::t!("toolbar.fit", locale = "en"), "Fit");
        assert_eq!(rust_i18n::t!("toolbar.fit", locale = "es"), "Encuadrar");
        assert_eq!(rust_i18n::t!("status.loading", locale = "es", file = "a.gds"), "Cargando a.gds …");
    }
}
