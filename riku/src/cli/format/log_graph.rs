//! `riku log --graph`: el historial con el grafo de ramas a la izquierda.
//!
//! ```text
//! ● 7be2104 (HEAD, main)  ajuste de W en M5
//! │   carlos · 2026-09-27 14:32
//! │   op_amp.sch  1 componente modificado
//! ○ 51c0de2 [merge]  Merge rama layout
//! ├─╮
//! │ ● 9a0f1e3 (layout)  pad nuevo en metal1
//! │ │   chip.gds  2 componentes añadidos
//! ● │ e02a4b1  valor de R2
//! ├─╯
//! ● 9d1f00c  inicial
//! ```
//!
//! Lee las filas del motor ([`crate::core::analysis::graph`]). Cada celda de
//! una línea de transición se arma con las direcciones que conecta (arriba,
//! abajo, izquierda, derecha) y de ahí sale el carácter, así cualquier cruce
//! se dibuja bien. Colores ANSI por carril solo si la salida es una terminal
//! (y sin `NO_COLOR`; `CLICOLOR_FORCE=1` los fuerza); `--color always|never` manda sobre todo eso.

use super::common::{detail_lines, format_counts, warning_lines};
use super::log_text::{first_line, format_refs, format_timestamp};
use crate::core::analysis::graph::GraphRow;
use crate::core::analysis::log::{LogCommit, LogReport};
use crate::core::analysis::summary::DetailLevel;

/// Cómo se dibuja el grafo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// Caracteres de dibujo Unicode; si no, ASCII (`* | / \ -`).
    pub unicode: bool,
    /// Colores ANSI por carril.
    pub color: bool,
}

impl Style {
    /// Unicode salvo `--ascii` o `RIKU_ASCII=1`; color según `--color` (ver
    /// [`super::color`]).
    pub fn detect(ascii: bool) -> Self {
        let env = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
        Self { unicode: !ascii && !env("RIKU_ASCII"), color: super::color::enabled() }
    }
}

pub fn print(report: &LogReport, level: DetailLevel, style: Style) {
    for w in &report.warnings {
        eprintln!("[!] {w}");
    }
    if report.commits.is_empty() {
        println!("{}", crate::i18n::tr!("log.no_commits"));
        return;
    }
    for line in render(&report.commits, level, style) {
        println!("{line}");
    }
}

/// Las líneas del log con el grafo, sin imprimirlas.
pub fn render(commits: &[LogCommit], level: DetailLevel, style: Style) -> Vec<String> {
    let rows: Vec<&GraphRow> = commits.iter().filter_map(|c| c.graph.as_ref()).collect();
    if rows.len() != commits.len() {
        return Vec::new();
    }
    let g = Glyphs::new(style);

    let mut out = Vec::new();
    for (c, row) in commits.iter().zip(&rows) {
        // Ancho de esta fila (como `git log --graph`): un tramo lineal no
        // hereda el ancho de la zona más ramificada del historial.
        let width = std::iter::once(row.column)
            .chain(row.passing.iter().map(|p| p.0))
            .chain(row.edges.iter().flat_map(|e| [e.0, e.1]))
            .max()
            .map_or(1, |m| m + 1);
        // Línea del commit: el nodo y las ramas que pasan.
        let mut cells = vec![Cell::default(); width];
        for &(col, lane) in &row.passing {
            cells[col] = Cell { text: g.vertical, lane: Some(lane) };
        }
        cells[row.column] = Cell { text: if c.is_merge { g.merge } else { g.node }, lane: Some(row.lane) };
        let refs = if c.refs.is_empty() { String::new() } else { format!(" {}", format_refs(&c.refs)) };
        let merge = if c.is_merge { crate::i18n::tr!("log.merge_tag") } else { String::new() };
        out.push(format!("{}{}{refs}{merge}  {}", g.paint(&cells, true), c.info.short_id, first_line(&c.info.message)));

        // Líneas de detalle: siguen las ramas que salen de esta fila.
        let gutter = g.paint(&continuation(row, width, &g), true);
        let mut text = vec![format!("  {} · {}", c.info.author, format_timestamp(c.info.timestamp))];
        if !c.is_merge {
            for f in &c.files {
                text.push(format!("  {}  {}", f.path, format_counts(f, true)));
                text.extend(warning_lines(f, "      "));
                if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
                    for d in &f.details {
                        text.extend(detail_lines(d, "      "));
                    }
                }
            }
        }
        for t in text {
            out.push(format!("{gutter}{t}"));
        }

        // Transición: solo si alguna rama cambia de columna.
        if row.edges.iter().any(|e| e.0 != e.1) {
            out.push(g.paint(&transition(row, width, &g), false).trim_end().to_string());
        }
    }
    out
}

/// Una celda del grafo: su texto y el carril que le da color.
#[derive(Clone, Default)]
struct Cell {
    text: &'static str,
    lane: Option<usize>,
}

/// Debajo del commit: una vertical por cada rama que sale de la fila; `┆` si
/// la rama del nodo termina por `-n`.
fn continuation(row: &GraphRow, width: usize, g: &Glyphs) -> Vec<Cell> {
    let mut cells = vec![Cell::default(); width];
    for &(from, _, lane) in &row.edges {
        cells[from] = Cell { text: g.vertical, lane: Some(lane) };
    }
    if row.truncated && cells[row.column].lane.is_none() {
        cells[row.column] = Cell { text: g.cut, lane: Some(row.lane) };
    }
    cells
}

const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

/// Entre esta fila y la siguiente. Las columnas del grafo van en las
/// posiciones pares; las impares solo llevan tramos horizontales.
fn transition(row: &GraphRow, width: usize, g: &Glyphs) -> Vec<Cell> {
    let n = 2 * width - 1;
    let mut dirs = vec![0u8; n];
    let mut lane: Vec<Option<usize>> = vec![None; n];
    // Las verticales primero: su color gana en los cruces.
    let mut edges = row.edges.clone();
    edges.sort_by_key(|e| e.0 == e.1);
    for &(a, b, l) in edges.iter().rev() {
        let (a, b) = (2 * a, 2 * b);
        if a == b {
            dirs[a] |= UP | DOWN;
            lane[a].get_or_insert(l);
            continue;
        }
        dirs[a] |= UP | if b > a { RIGHT } else { LEFT };
        dirs[b] |= DOWN | if b > a { LEFT } else { RIGHT };
        for k in a.min(b) + 1..a.max(b) {
            dirs[k] |= LEFT | RIGHT;
        }
        for k in a.min(b)..=a.max(b) {
            lane[k].get_or_insert(l);
        }
    }
    // Sin separador entre columnas: cada celda ya es una posición.
    dirs.iter().zip(lane).map(|(&d, l)| Cell { text: g.junction(d), lane: l }).collect()
}

struct Glyphs {
    style: Style,
    node: &'static str,
    merge: &'static str,
    vertical: &'static str,
    cut: &'static str,
}

impl Glyphs {
    fn new(style: Style) -> Self {
        if style.unicode {
            Self { style, node: "●", merge: "○", vertical: "│", cut: "┆" }
        } else {
            Self { style, node: "*", merge: "*", vertical: "|", cut: ":" }
        }
    }

    /// El carácter que une las direcciones `d`.
    fn junction(&self, d: u8) -> &'static str {
        let (u, dn, l, r) = (d & UP != 0, d & DOWN != 0, d & LEFT != 0, d & RIGHT != 0);
        if self.style.unicode {
            match (u, dn, l, r) {
                (false, false, false, false) => " ",
                (true, true, false, false) => "│",
                (false, false, _, _) => "─",
                (true, false, true, false) => "╯",
                (true, false, false, true) => "╰",
                (false, true, true, false) => "╮",
                (false, true, false, true) => "╭",
                (true, true, false, true) => "├",
                (true, true, true, false) => "┤",
                (false, true, true, true) => "┬",
                (true, false, true, true) => "┴",
                (true, true, true, true) => "┼",
                _ => "│",
            }
        } else {
            match (u, dn, l, r) {
                (false, false, false, false) => " ",
                (true, true, true, true) => "+",
                (true, true, _, _) => "|",
                (false, false, _, _) => "-",
                (true, false, true, _) | (false, true, false, true) => "/",
                (true, false, false, true) | (false, true, true, false) => "\\",
                _ => "|",
            }
        }
    }

    /// Las celdas como texto. `spaced`: cada columna ocupa dos posiciones
    /// (la celda y un espacio), como en la línea de un commit y en las de
    /// detalle; las de transición ya traen las dos.
    fn paint(&self, cells: &[Cell], spaced: bool) -> String {
        const PALETTE: [u8; 6] = [36, 32, 33, 35, 34, 31];
        let mut s = String::new();
        for c in cells {
            let text = if c.text.is_empty() { " " } else { c.text };
            match (self.style.color, c.lane) {
                (true, Some(l)) if text != " " => s.push_str(&format!("\x1b[{}m{text}\x1b[0m", PALETTE[l % PALETTE.len()])),
                _ => s.push_str(text),
            }
            if spaced {
                s.push(' ');
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analysis::graph::{layout, tests::dag};
    use crate::core::domain::git_types::CommitInfo;

    fn commits(spec: &str) -> Vec<LogCommit> {
        let d = dag(spec);
        d.iter()
            .zip(layout(&d))
            .map(|((oid, parents), row)| LogCommit {
                info: CommitInfo {
                    oid: oid.clone(),
                    short_id: oid.clone(),
                    message: format!("msg {oid}"),
                    author: "t".into(),
                    timestamp: 0,
                },
                parents: parents.clone(),
                refs: Vec::new(),
                is_merge: parents.len() > 1,
                files: Vec::new(),
                graph: Some(row),
            })
            .collect()
    }

    fn plain(spec: &str, unicode: bool) -> String {
        render(&commits(spec), DetailLevel::default(), Style { unicode, color: false }).join("\n")
    }

    #[test]
    fn branch_and_merge_in_unicode() {
        // Cada fila con su ancho: la del merge ya abre la segunda columna.
        let expected = "\
○   m [merge]  msg m
│     t · unknown
├─╮
● │ a  msg a
│ │   t · unknown
│ ● f  msg f
│ │   t · unknown
├─╯
● b  msg b
    t · unknown";
        assert_eq!(plain("m:a f, a:b, f:b, b:", true), expected);
    }

    #[test]
    fn ascii_uses_plain_characters() {
        let out = plain("m:a f, a:b, f:b, b:", false);
        // Sin caracteres de dibujo Unicode (el `·` es del texto del log).
        assert!(!out.chars().any(|c| matches!(c, '\u{2500}'..='\u{257f}' | '●' | '○')), "{out}");
        assert!(out.contains("|-\\"), "{out}");
        assert!(out.contains("|-/"), "{out}");
    }

    #[test]
    fn cut_branch_is_marked() {
        // -n cortó a los padres de a.
        let out = plain("a:b", true);
        assert!(out.contains('┆'), "{out}");
    }

    #[test]
    fn colors_only_when_asked() {
        let c = commits("m:a f, a:b, f:b, b:");
        let colored = render(&c, DetailLevel::default(), Style { unicode: true, color: true }).join("\n");
        assert!(colored.contains("\x1b["));
        assert!(!plain("m:a f, a:b, f:b, b:", true).contains("\x1b["));
    }
}
