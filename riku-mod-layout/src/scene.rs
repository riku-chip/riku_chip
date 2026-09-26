
use gdstk_rs::{Anchor, GdsTag, Library, Point2D};

use crate::labels::{flatten_labels, FlatLabel};

/// Comandos de dibujo de una cell, antes de pasar a la escena neutra del visor.
///
/// Solo dos variantes: `Polygon` y `Label`. gdstk polygoniza paths/rects
/// internamente al llamar `cell.get_polygons().build()`, asi que llegan ya
/// como Polygon con su geometria gruesa bakeada.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Polygon {
        tag: GdsTag,
        points: Vec<Point2D>,
    },
    Label {
        tag: GdsTag,
        text: String,
        origin: Point2D,
        /// Posicion del texto respecto a `origin` (PRESENTATION del GDS).
        anchor: Anchor,
    },
}

impl DrawCommand {
    pub fn tag(&self) -> GdsTag {
        match self {
            Self::Polygon { tag, .. } | Self::Label { tag, .. } => *tag,
        }
    }
}

/// Poligonos de toda la jerarquia de `cell` y los labels de todas sus
/// sub-cells, transformados a coordenadas de `cell` (igual que KLayout
/// muestra una cell jerarquica). Primero los poligonos, en el orden del
/// archivo; despues los labels.
pub fn draw_commands(lib: &Library, cell: &gdstk_rs::Cell<'_>) -> Vec<DrawCommand> {
    let flattened = cell.get_polygons().build();
    let mut commands: Vec<DrawCommand> = flattened
        .polygons()
        .map(|polygon| DrawCommand::Polygon {
            tag: GdsTag { layer: polygon.layer(), datatype: polygon.datatype() },
            points: polygon.points().collect(),
        })
        .collect();
    commands.extend(
        flatten_labels(lib, cell)
            .into_iter()
            .map(|FlatLabel { tag, text, origin, anchor }| DrawCommand::Label { tag, text, origin, anchor }),
    );
    commands
}
