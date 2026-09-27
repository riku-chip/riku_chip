//! Recompilar cuando cambian las traducciones: `rust_i18n::i18n!` incrusta
//! `locales/*.yml` al compilar, pero cargo no sabe que dependen de ellos
//! (sin esto, agregar `pt.yml` o corregir un texto no se veía hasta tocar
//! un `.rs`).
fn main() {
    println!("cargo:rerun-if-changed=locales");
}
