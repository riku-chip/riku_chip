//! Módulos de formato de este ejecutable.
//!
//! **Único lugar que sabe qué formatos existen.** El núcleo (`log`, `status`,
//! diff de commits), la CLI y el visor reciben el [`Registry`] que arma
//! [`registry`] y no nombran ningún formato. Cada módulo se compila con su
//! feature de Cargo (`xschem`, `layout`, `spice`), así se puede armar un `riku` con
//! solo algunos formatos.

#[cfg(any(feature = "xschem", feature = "layout", feature = "spice"))]
use std::sync::Arc;

use riku_kernel::Registry;

#[cfg(feature = "layout")]
pub mod layout;
#[cfg(feature = "spice")]
pub mod spice;
#[cfg(feature = "xschem")]
pub mod xschem;
#[cfg(all(feature = "xschem", feature = "gui"))]
pub mod xschem_view;
// Solo std: el diagnóstico de PDK de `riku doctor` funciona sin el módulo.
pub mod xschem_pdk;

/// Registro con todos los módulos compilados en este ejecutable.
pub fn registry() -> Registry {
    #[allow(unused_mut)]
    let mut r = Registry::new();
    #[cfg(feature = "xschem")]
    r.add(Arc::new(xschem::XschemModule::new()));
    #[cfg(feature = "layout")]
    r.add(Arc::new(layout::LayoutModule::new()));
    #[cfg(feature = "spice")]
    r.add(Arc::new(spice::WaveformModule::new()));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use riku_kernel::FileFormat;

    #[test]
    #[cfg(all(feature = "xschem", feature = "layout"))]
    fn every_module_is_registered_and_detects_its_format() {
        let r = registry();
        assert_eq!(r.detect_format(b"v {xschem version=3.4.5 file_version=1.2}\n"), FileFormat::Xschem);
        assert_eq!(r.detect_format(&[0x00, 0x06, 0x00, 0x02, 0x02, 0x58]), FileFormat::Gds);
        assert_eq!(r.detect_format(b"%SEMI-OASIS\r\n\x01"), FileFormat::Gds);
        assert_eq!(r.detect_format(b"<svg/>"), FileFormat::Unknown);
        assert_eq!(r.for_path("a/b/amp.sch").map(|m| m.info().format), Some(FileFormat::Xschem));
        assert_eq!(r.for_path("chip.OAS").map(|m| m.info().format), Some(FileFormat::Gds));
        assert!(r.for_path("notas.txt").is_none());
        let mut ext = vec!["sch", "gds", "oas"];
        if cfg!(feature = "spice") {
            ext.push("raw");
        }
        assert_eq!(r.extensions(), ext);
    }
}
