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
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            cosmetic_threshold_um2: gds_renderer::DEFAULT_COSMETIC_THRESHOLD_UM2,
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
        Box::new(GdsDriver::with_threshold(cfg.cosmetic_threshold_um2)),
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
