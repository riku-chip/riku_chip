use crate::adapters::gds_driver::GdsDriver;
use crate::adapters::xschem_driver::XschemDriver;
use crate::core::domain::driver::RikuDriver;

pub fn get_drivers() -> Vec<Box<dyn RikuDriver>> {
    vec![Box::new(XschemDriver::new()), Box::new(GdsDriver::new())]
}

pub fn get_driver_for(filename: &str) -> Option<Box<dyn RikuDriver>> {
    get_drivers()
        .into_iter()
        .find(|driver| driver.can_handle(filename))
}
