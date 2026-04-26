//! Wrapper genérico para serialización JSON con campo `schema` versionado.
//!
//! Cada reporte público (status, log, ...) se envuelve en `Envelope<T>` antes
//! de pasar a `serde_json` para garantizar que el JSON externo siempre lleva
//! `"schema": "riku-<comando>/v<n>"`.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Envelope<'a, T: Serialize> {
    pub schema: &'static str,
    #[serde(flatten)]
    pub inner: &'a T,
}

impl<'a, T: Serialize> Envelope<'a, T> {
    pub fn new(schema: &'static str, inner: &'a T) -> Self {
        Self { schema, inner }
    }
}
