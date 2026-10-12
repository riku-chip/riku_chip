//! Idioma de los textos que este crate muestra al usuario: el resumen de una
//! celda, las capas y cambios de un diff y los avisos. Inglés por defecto;
//! quien lo usa (`riku`, según `RIKU_LANG` o el visor) lo cambia con
//! [`set_spanish`]. Los textos dependen del idioma, así que entra en las
//! claves de las cachés que los guardan ([`tag`]).

use std::sync::atomic::{AtomicBool, Ordering};

static SPANISH: AtomicBool = AtomicBool::new(false);

/// Textos en español (`true`) o en inglés.
pub fn set_spanish(on: bool) {
    SPANISH.store(on, Ordering::Relaxed);
}

pub fn spanish() -> bool {
    SPANISH.load(Ordering::Relaxed)
}

/// `en` o `es` según el idioma.
pub(crate) fn pick<'a>(en: &'a str, es: &'a str) -> &'a str {
    if spanish() {
        es
    } else {
        en
    }
}

/// Cómo se nombra cada lado de un diff en los avisos (`before: …`).
pub(crate) fn before() -> &'static str {
    pick("before", "antes")
}

pub(crate) fn after() -> &'static str {
    pick("after", "después")
}

/// El idioma, para las claves de caché.
pub(crate) fn tag() -> &'static str {
    pick("en", "es")
}

/// `t!("inglés {x}", "español {x}", …)`: un texto con formato en el idioma
/// elegido (los dos con los mismos argumentos).
macro_rules! t {
    ($en:literal, $es:literal $(, $arg:expr)* $(,)?) => {
        if $crate::lang::spanish() {
            format!($es $(, $arg)*)
        } else {
            format!($en $(, $arg)*)
        }
    };
}
pub(crate) use t;
