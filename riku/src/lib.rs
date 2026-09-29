pub mod cli;
pub mod core;
pub mod export;
#[cfg(feature = "gui")]
pub mod gui;
pub mod i18n;
#[cfg(all(feature = "xschem", feature = "layout"))]
pub mod lvs;

// Textos de la CLI y del visor (locales/*.yml); inglés si falta una traducción.
rust_i18n::i18n!("locales", fallback = "en");
pub mod modules;
pub mod text;
