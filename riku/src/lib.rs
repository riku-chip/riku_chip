pub mod cli;
pub mod core;
#[cfg(feature = "gui")]
pub mod gui;

// Textos del visor (locales/gui.yml); inglés si falta una traducción.
#[cfg(feature = "gui")]
rust_i18n::i18n!("locales", fallback = "en");
pub mod modules;
