use crate::adapters::gds_driver::GdsDriver;
use crate::adapters::xschem_driver::XschemDriver;
use crate::core::domain::driver::RikuDriver;

/// Configuracion opcional para construir drivers. Cada driver toma lo que le
/// aplica e ignora el resto. Default reusa los defaults de cada driver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriverConfig {
    /// Umbral en µm² para clasificar un cambio GDS como cosmetico (sub-DRC).
    /// Solo afecta a `GdsDriver`. Ver `gds_renderer::DEFAULT_COSMETIC_THRESHOLD_UM2`.
    pub cosmetic_threshold_um2: f64,
    /// Cache en disco de diffs de layouts grandes (`--no-cache` la apaga).
    pub use_cache: bool,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            cosmetic_threshold_um2: gds_renderer::DEFAULT_COSMETIC_THRESHOLD_UM2,
            use_cache: true,
        }
    }
}

pub fn get_drivers() -> Vec<Box<dyn RikuDriver>> {
    get_drivers_with_config(&DriverConfig::default())
}

pub fn get_driver_for(filename: &str) -> Option<Box<dyn RikuDriver>> {
    get_driver_for_with_config(filename, &DriverConfig::default())
}

pub fn get_drivers_with_config(cfg: &DriverConfig) -> Vec<Box<dyn RikuDriver>> {
    vec![
        Box::new(XschemDriver::new()),
        Box::new(GdsDriver::with_config(cfg.cosmetic_threshold_um2, cfg.use_cache)),
    ]
}

pub fn get_driver_for_with_config(
    filename: &str,
    cfg: &DriverConfig,
) -> Option<Box<dyn RikuDriver>> {
    get_drivers_with_config(cfg)
        .into_iter()
        .find(|driver| driver.can_handle(filename))
}

/// Formato de un contenido según su firma: se le pregunta a cada driver
/// (el núcleo no conoce las firmas). `Unknown` si ninguno lo reconoce.
pub fn detect_format(content: &[u8]) -> crate::core::domain::models::FileFormat {
    get_drivers()
        .iter()
        .find(|d| d.detect(content))
        .map_or(crate::core::domain::models::FileFormat::Unknown, |d| d.format())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::models::FileFormat;

    #[test]
    fn each_driver_recognizes_its_own_signature() {
        assert_eq!(detect_format(b"v {xschem version=3.4.5 file_version=1.2}\n"), FileFormat::Xschem);
        assert_eq!(detect_format(&[0x00, 0x06, 0x00, 0x02, 0x02, 0x58]), FileFormat::Gds);
        assert_eq!(detect_format(b"%SEMI-OASIS\r\n\x01"), FileFormat::Gds);
        assert_eq!(detect_format(b"<svg/>"), FileFormat::Unknown);
        assert_eq!(detect_format(&[]), FileFormat::Unknown);
    }

    #[test]
    fn the_driver_is_chosen_by_extension() {
        assert_eq!(get_driver_for("a/b/amp.sch").map(|d| d.format()), Some(FileFormat::Xschem));
        assert_eq!(get_driver_for("chip.OAS").map(|d| d.format()), Some(FileFormat::Gds));
        assert!(get_driver_for("notas.txt").is_none());
    }
}
