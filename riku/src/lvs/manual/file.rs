//! El archivo de vínculos (`lvs/<celda>.toml`): leerlo, escribirlo y su formato.

use super::*;
use serde::{Deserialize, Serialize};
use crate::i18n::tr;

/// El archivo de vínculos de un par.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapFile {
    pub schema: String,
    pub schematic: String,
    pub layout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, rename = "bind")]
    pub binds: Vec<Bind>,
}

/// Un transistor del esquemático y los del layout que lo forman (sus dedos).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bind {
    pub schematic: String,
    pub layout: Vec<LayoutRef>,
}

/// Un transistor del layout: su modelo y el centro de su compuerta (µm, en
/// coordenadas de la celda comparada). Si está dibujado en una sub-celda,
/// también esa celda y el punto en sus coordenadas: así se lo reencuentra
/// aunque se mueva su instancia.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutRef {
    pub model: String,
    pub at: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<[f64; 2]>,
}

impl LayoutRef {
    /// La referencia a un transistor del layout tal como está ahora.
    pub fn of(d: &LayDevice) -> Self {
        let sub = d.cell.is_some();
        LayoutRef { model: d.model.clone(), at: [d.at.0, d.at.1], cell: d.cell.clone(), local: sub.then_some([d.local.0, d.local.1]) }
    }
}

impl MapFile {
    pub fn new(schematic: &str, layout: &str, cell: Option<&str>) -> Self {
        MapFile { schema: SCHEMA.into(), schematic: schematic.into(), layout: layout.into(), cell: cell.map(str::to_string), binds: Vec::new() }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let map: MapFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if map.schema != SCHEMA {
            return Err(tr!("lvs_map.bad_schema", found = map.schema, want = SCHEMA));
        }
        Ok(map)
    }

    /// Los vínculos en orden de nombre (`M2` antes que `M10`).
    pub fn sort(&mut self) {
        self.binds.sort_by(|a, b| natural(&a.schematic).cmp(&natural(&b.schematic)));
    }

    /// Escribe el archivo sin dejarlo nunca a medias: a uno temporal al lado
    /// y después se renombra (en el mismo disco, el cambio es de una vez).
    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }

    /// El archivo como texto: un vínculo por bloque, en orden de nombre,
    /// para que el diff de un commit se lea bien.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Qué transistor del esquemático es cuál del layout (riku: LVS manual).\n");
        out.push_str("# Cada transistor del layout: su modelo y un punto de su compuerta, en µm de la celda.\n");
        out.push_str(&format!("schema = {:?}\nschematic = {:?}\nlayout = {:?}\n", self.schema, self.schematic, self.layout));
        if let Some(c) = &self.cell {
            out.push_str(&format!("cell = {c:?}\n"));
        }
        let mut binds = self.binds.clone();
        binds.sort_by(|a, b| natural(&a.schematic).cmp(&natural(&b.schematic)));
        for b in &binds {
            out.push_str(&format!("\n[[bind]]\nschematic = {:?}\nlayout = [\n", b.schematic));
            let mut refs = b.layout.clone();
            refs.sort_by(|a, b| (a.at[0], a.at[1]).partial_cmp(&(b.at[0], b.at[1])).unwrap_or(std::cmp::Ordering::Equal));
            for r in refs {
                let sub = match (&r.cell, r.local) {
                    (Some(c), Some(l)) => format!(", cell = {c:?}, local = [{:.3}, {:.3}]", l[0], l[1]),
                    _ => String::new(),
                };
                out.push_str(&format!("  {{ model = {:?}, at = [{:.3}, {:.3}]{sub} }},\n", r.model, r.at[0], r.at[1]));
            }
            out.push_str("]\n");
        }
        out
    }
}

/// Orden natural: `M2` antes que `M10`.
pub(super) fn natural(s: &str) -> (String, u64, String) {
    let digits = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let end = s[digits..].find(|c: char| !c.is_ascii_digit()).map_or(s.len(), |e| digits + e);
    (s[..digits].to_string(), s[digits..end].parse().unwrap_or(0), s[end..].to_string())
}
