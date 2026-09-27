//! Grafo del historial: ubica cada commit en un carril y dice por dónde pasa
//! cada rama entre una fila y la siguiente.
//!
//! No sabe de formatos (es Git, no un módulo) ni de cómo se dibuja: la
//! terminal (`riku log --graph`) y el visor leen las mismas [`GraphRow`].
//!
//! El historial es un DAG; las filas van en orden topológico (cada hijo antes
//! que sus padres). Se recorre una vez manteniendo las columnas activas, cada
//! una esperando un commit (el próximo padre de esa rama):
//!
//! - Las columnas que esperan al commit se juntan en él: su columna es la de
//!   más a la izquierda.
//! - El primer padre hereda la columna y el carril (color) del commit: la
//!   línea principal de una rama queda recta y de un solo color.
//! - Cada padre extra (merge) abre una columna nueva a la derecha del nodo,
//!   con un carril nuevo. Si otra columna ya esperaba a ese padre, las dos se
//!   juntan al llegar a él.
//! - Las columnas que se vacían se compactan hacia la izquierda.
//!
//! Es secuencial por naturaleza (cada fila depende de la anterior) y lineal
//! en la cantidad de commits por la de columnas activas: miles de commits son
//! microsegundos, así que no se reparte entre hilos.

use std::collections::{HashMap, HashSet};

/// Una fila del grafo: un commit.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphRow {
    /// Columna del nodo (0 = izquierda).
    pub column: usize,
    /// Carril del nodo: identidad estable de su rama visual (para el color).
    pub lane: usize,
    /// Otras ramas que pasan por esta fila sin tocar el nodo: (columna, carril).
    pub passing: Vec<(usize, usize)>,
    /// Tramos entre esta fila y la siguiente: (columna aquí, columna en la
    /// siguiente, carril). Vertical si las dos columnas coinciden; oblicuo si
    /// la rama se abre desde el nodo, se une a otro nodo o se corre.
    pub edges: Vec<(usize, usize, usize)>,
    /// Tiene padres fuera de la ventana cargada (`-n`): su rama termina acá.
    pub truncated: bool,
}

/// Una columna activa: la rama espera a `oid` y pinta con `lane`. `from` es
/// la columna donde nace el tramo en la fila actual.
#[derive(Clone)]
struct Slot {
    oid: String,
    lane: usize,
    from: usize,
}

/// Ubica `commits` (oid y padres, en orden topológico) en carriles. Los
/// padres que no están en la lista se consideran fuera de la ventana.
pub fn layout(commits: &[(String, Vec<String>)]) -> Vec<GraphRow> {
    let present: HashSet<&str> = commits.iter().map(|(oid, _)| oid.as_str()).collect();
    let mut active: Vec<Slot> = Vec::new();
    let mut next_lane = 0usize;
    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());

    for (oid, parents) in commits {
        // Columnas en esta fila: las que siguen y el nodo, donde se juntan
        // las que lo esperaban.
        let mut row_cols: Vec<Option<Slot>> = Vec::with_capacity(active.len() + 1);
        let mut node: Option<(usize, usize)> = None; // (columna, carril)
        let mut incoming: Vec<(usize, usize, usize)> = Vec::new();
        for slot in active.drain(..) {
            if slot.oid == *oid {
                let col = match node {
                    Some((c, _)) => c,
                    None => {
                        node = Some((row_cols.len(), slot.lane));
                        row_cols.push(None);
                        row_cols.len() - 1
                    }
                };
                incoming.push((slot.from, col, slot.lane));
            } else {
                incoming.push((slot.from, row_cols.len(), slot.lane));
                row_cols.push(Some(slot));
            }
        }
        // Una punta de rama (sin hijos cargados) abre una columna a la derecha.
        let (column, lane) = node.unwrap_or_else(|| {
            row_cols.push(None);
            let l = next_lane;
            next_lane += 1;
            (row_cols.len() - 1, l)
        });
        if lane >= next_lane {
            next_lane = lane + 1;
        }
        // Los tramos que llegan a esta fila son los de la fila anterior.
        if let Some(prev) = rows.last_mut() {
            prev.edges = incoming;
        }

        let passing: Vec<(usize, usize)> =
            row_cols.iter().enumerate().filter_map(|(c, s)| s.as_ref().map(|s| (c, s.lane))).collect();
        let in_window: Vec<&String> = parents.iter().filter(|p| present.contains(p.as_str())).collect();
        let truncated = in_window.len() < parents.len();

        // Columnas que salen de esta fila: el primer padre en la del nodo, los
        // padres extra a su derecha (nacen en el nodo), después el resto.
        let mut out: Vec<Slot> = Vec::with_capacity(row_cols.len() + in_window.len());
        for (c, s) in row_cols.into_iter().enumerate() {
            match s {
                Some(mut s) => {
                    s.from = c;
                    out.push(s);
                }
                None => {
                    for (k, p) in in_window.iter().enumerate() {
                        let l = if k == 0 {
                            lane
                        } else {
                            next_lane += 1;
                            next_lane - 1
                        };
                        out.push(Slot { oid: (*p).clone(), lane: l, from: column });
                    }
                }
            }
        }
        active = out;
        rows.push(GraphRow { column, lane, passing, edges: Vec::new(), truncated });
    }
    rows
}

/// Reescribe los padres de los commits visibles al ancestro visible más
/// cercano, pasando por los ocultos (lo que Git llama simplificación del
/// historial): con `--paths` se omiten los commits que no tocan esos
/// archivos, y sin esto sus hijos quedarían desconectados. Los padres fuera
/// de `all` quedan como están (fuera de la ventana). `all` va en orden
/// topológico; devuelve los visibles en ese orden, sin padres repetidos.
pub fn simplify(all: &[(String, Vec<String>)], visible: &HashSet<String>) -> Vec<(String, Vec<String>)> {
    let loaded: HashMap<&str, &Vec<String>> = all.iter().map(|(o, p)| (o.as_str(), p)).collect();
    // Para cada commit, los visibles que "lo representan": él mismo si es
    // visible; si no, lo que representen sus padres. Los padres se procesan
    // antes que los hijos (orden topológico invertido).
    let mut reach: HashMap<&str, Vec<String>> = HashMap::with_capacity(all.len());
    let through = |p: &String, reach: &HashMap<&str, Vec<String>>| -> Vec<String> {
        match (loaded.contains_key(p.as_str()), reach.get(p.as_str())) {
            (true, Some(r)) => r.clone(),
            _ => vec![p.clone()],
        }
    };
    for (oid, parents) in all.iter().rev() {
        let r = if visible.contains(oid) {
            vec![oid.clone()]
        } else {
            let mut acc: Vec<String> = Vec::new();
            for p in parents {
                for x in through(p, &reach) {
                    if !acc.contains(&x) {
                        acc.push(x);
                    }
                }
            }
            acc
        };
        reach.insert(oid.as_str(), r);
    }
    all.iter()
        .filter(|(oid, _)| visible.contains(oid))
        .map(|(oid, parents)| {
            let mut out: Vec<String> = Vec::new();
            for p in parents {
                for x in through(p, &reach) {
                    if !out.contains(&x) {
                        out.push(x);
                    }
                }
            }
            (oid.clone(), out)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// DAG desde texto: `"a:b c, b:d, c:d, d:"` (hijos primero).
    pub(crate) fn dag(spec: &str) -> Vec<(String, Vec<String>)> {
        spec.split(',')
            .map(|item| {
                let (oid, parents) = item.trim().split_once(':').unwrap();
                (oid.to_string(), parents.split_whitespace().map(str::to_string).collect())
            })
            .collect()
    }

    /// Comprueba que el grafo dibuja exactamente el DAG: siguiendo los tramos
    /// desde cada nodo se llega a sus padres cargados y a nada más; nunca
    /// dos ramas en la misma columna de una fila.
    pub(crate) fn check(commits: &[(String, Vec<String>)], rows: &[GraphRow]) {
        assert_eq!(rows.len(), commits.len());
        let row_of: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, (o, _))| (o.as_str(), i)).collect();
        for (i, row) in rows.iter().enumerate() {
            let mut cols: Vec<usize> = row.passing.iter().map(|(c, _)| *c).collect();
            cols.push(row.column);
            let n = cols.len();
            cols.sort_unstable();
            cols.dedup();
            assert_eq!(cols.len(), n, "fila {i}: dos ramas en la misma columna");
            // Cada rama que pasa llega desde arriba (salvo en la primera fila).
            if i > 0 {
                for (c, _) in &row.passing {
                    assert!(rows[i - 1].edges.iter().any(|e| e.1 == *c), "fila {i}: la rama de la columna {c} no viene de arriba");
                }
            }
        }
        for (i, (oid, parents)) in commits.iter().enumerate() {
            let expected: HashSet<usize> = parents.iter().filter_map(|p| row_of.get(p.as_str()).copied()).collect();
            assert_eq!(rows[i].truncated, expected.len() < parents.iter().collect::<HashSet<_>>().len(), "{oid}: truncated");
            // Seguir cada tramo que sale del nodo hasta que entra en otro nodo.
            let mut reached = HashSet::new();
            for e in rows[i].edges.iter().filter(|e| e.0 == rows[i].column) {
                let (mut r, mut c) = (i + 1, e.1);
                loop {
                    assert!(r < rows.len(), "{oid}: un tramo se sale del grafo");
                    if rows[r].column == c {
                        reached.insert(r);
                        break;
                    }
                    let next = rows[r].edges.iter().filter(|x| x.0 == c).collect::<Vec<_>>();
                    assert_eq!(next.len(), 1, "{oid}: en la fila {r} la columna {c} no sigue a un solo lugar");
                    c = next[0].1;
                    r += 1;
                }
            }
            assert_eq!(reached, expected, "{oid}: los tramos no llegan a sus padres");
        }
    }

    fn run(spec: &str) -> Vec<GraphRow> {
        let d = dag(spec);
        let rows = layout(&d);
        check(&d, &rows);
        rows
    }

    #[test]
    fn linear_history_is_one_straight_lane() {
        let rows = run("a:b, b:c, c:");
        assert!(rows.iter().all(|r| r.column == 0 && r.lane == 0 && r.passing.is_empty()));
        assert_eq!(rows[0].edges, vec![(0, 0, 0)]);
        assert!(rows[2].edges.is_empty());
    }

    #[test]
    fn branch_and_merge() {
        // m une a (rama principal) y f (rama lateral); las dos salen de b.
        let rows = run("m:a f, a:b, f:b, b:");
        assert_eq!(rows[0].column, 0);
        // El merge abre una columna a la derecha para su segundo padre.
        assert_eq!(rows[0].edges, vec![(0, 0, 0), (0, 1, 1)]);
        // La rama lateral va en la columna 1 y se junta en b.
        assert_eq!((rows[2].column, rows[2].lane), (1, 1));
        assert_eq!(rows[2].edges, vec![(0, 0, 0), (1, 0, 1)]);
    }

    #[test]
    fn hard_cases_follow_the_dag() {
        for spec in [
            // Merge de merge.
            "m2:m1 g, m1:a f, g:a, a:b, f:b, b:",
            // Octopus: tres padres.
            "o:a b c, a:r, b:r, c:r, r:",
            // Ramas cruzadas (criss-cross).
            "x:p q, y:q p, p:r, q:r, r:",
            // Dos raíces.
            "m:a z, a:, z:",
            // Dos puntas de rama que empiezan a la vez.
            "t1:a, t2:a, a:",
            // Rama larga al costado mientras la principal avanza.
            "m:a1 f3, a1:a2, f3:f2, a2:a3, f2:f1, a3:b, f1:b, b:",
        ] {
            run(spec);
        }
    }

    #[test]
    fn window_cut_marks_truncated_branches() {
        // Con -n, b y c quedan fuera: a los tiene como padres.
        let rows = run("m:a f, a:b, f:c");
        assert!(rows[1].truncated && rows[2].truncated);
        assert!(!rows[0].truncated);
    }

    /// La historia real de gdstk (el submódulo: cientos de commits y decenas
    /// de merges) en orden topológico, como la lee `riku log --graph`.
    fn gdstk_history() -> Option<Vec<(String, Vec<String>)>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../external/gdstk");
        let repo = git2::Repository::open(path).ok()?;
        let mut walk = repo.revwalk().ok()?;
        walk.push_head().ok()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME).ok()?;
        let out: Vec<(String, Vec<String>)> = walk
            .filter_map(Result::ok)
            .filter_map(|oid| {
                let c = repo.find_commit(oid).ok()?;
                Some((oid.to_string(), c.parent_ids().map(|p| p.to_string()).collect()))
            })
            .collect();
        Some(out)
    }

    #[test]
    fn real_history_with_merges_follows_the_dag() {
        let Some(all) = gdstk_history() else { return };
        let merges = all.iter().filter(|(_, p)| p.len() > 1).count();
        // La CI baja el submódulo con un solo commit (clon superficial): sin
        // historia no hay nada que probar acá; los casos sintéticos siguen.
        if merges == 0 {
            eprintln!("historia de gdstk recortada ({} commits): se salta", all.len());
            return;
        }
        assert!(all.len() > 500 && merges > 20, "{} commits, {merges} merges", all.len());
        // Entera, cortada como con `-n 100`, y con un tercio de los commits
        // visibles (como con `--paths`).
        check(&all, &layout(&all));
        let window = all[..100].to_vec();
        check(&window, &layout(&window));
        let visible: HashSet<String> = all.iter().step_by(3).map(|(o, _)| o.clone()).collect();
        let shown = simplify(&all, &visible);
        assert_eq!(shown.len(), visible.len());
        check(&shown, &layout(&shown));
    }

    #[test]
    fn simplify_skips_hidden_commits() {
        // Visibles: m, f1, b. a1 y a2 están ocultos entre m y b.
        let d = dag("m:a1 f1, a1:a2, f1:b, a2:b, b:");
        let visible: HashSet<String> = ["m", "f1", "b"].iter().map(|s| s.to_string()).collect();
        let s = simplify(&d, &visible);
        assert_eq!(s, dag("m:b f1, f1:b, b:"));
        check(&s, &layout(&s));
        // Un padre fuera de la ventana queda como está.
        let d = dag("a:h, h:x");
        let visible: HashSet<String> = ["a"].iter().map(|s| s.to_string()).collect();
        assert_eq!(simplify(&d, &visible), dag("a:x"));
    }
}
