use gdstk_rs::GdsTag;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_svg_rgba(self) -> String {
        format!(
            "rgba({},{},{},{:.3})",
            self.r,
            self.g,
            self.b,
            self.a as f32 / 255.0
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pdk {
    Sky130,
    Gf180,
    Ihp,
    Generic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderConfig {
    pub width: u32,
    pub height: u32,
    pub pdk: Pdk,
    pub background: Option<Color>,
    pub show_labels: bool,
    pub include_layer_metadata: bool,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 1000,
            pdk: Pdk::Generic,
            background: Some(Color::rgba(18, 18, 24, 255)),
            show_labels: true,
            include_layer_metadata: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    pub tag: GdsTag,
    pub name: String,
    pub color: Color,
    pub visible: bool,
    pub order: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerCatalog {
    pub layers: Vec<LayerStyle>,
}

impl LayerCatalog {
    pub fn visible_layers(&self) -> impl Iterator<Item = &LayerStyle> {
        self.layers.iter().filter(|layer| layer.visible)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerStyle {
    pub tag: GdsTag,
    pub name: String,
    pub fill: Color,
    pub stroke: Color,
    pub opacity: f32,
    pub visible: bool,
    pub order: u32,
    pub hatch: Option<HatchPattern>,
}

impl LayerStyle {
    pub fn default_for(tag: GdsTag, name: String, order: u32, fill: Color) -> Self {
        Self {
            tag,
            name,
            fill,
            stroke: fill,
            opacity: 1.0,
            visible: true,
            order,
            hatch: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HatchPattern {
    Solid,
    Diagonal,
    Cross,
    Dots,
}
