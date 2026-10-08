//! El LVS dentro de `riku log --lvs` (cada commit contra su primer padre) y
//! de `riku status --lvs` (el working tree contra `HEAD`). Ver
//! `docs/dev/design-notes.md` (D11, D12).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{result_at, tools, Cache, CommitVersion, DiskVersion, Pair, StepResult, Tools, Tree};
use crate::core::analysis::log::LogReport;
use crate::core::analysis::lvs_types::{Delta, Discrepancy, LvsState, PairLvs, PairStatusLvs, Transition};
use crate::core::analysis::status::StatusReport;
use crate::core::analysis::summary::DetailLevel;
use crate::core::path_matcher::PathMatcher;
use crate::i18n::tr;

/// Los pares del proyecto en `root` (los de `.riku.toml` o, si no hay, por
/// nombre) que tocan alguno de `paths` (globs; vacío: todos), y los avisos de
/// emparejamiento ([`super::pairs_checked`]).
pub fn project_pairs(root: &Path, paths: &[String]) -> Result<(Vec<Pair>, Vec<String>), String> {
    let configured: Vec<Pair> = crate::core::config::load(Some(root))?
        .lvs
        .into_iter()
        .map(|c| Pair { schematic: c.schematic, layout: c.layout, cell: c.cell })
        .collect();
    let m = PathMatcher::new(paths);
    let (found, warnings) = super::pairs_checked(root, &configured);
    Ok((found.into_iter().filter(|p| m.matches(&p.schematic) || m.matches(&p.layout)).collect(), warnings))
}

fn state(r: &StepResult) -> LvsState {
    match r {
        StepResult::Done { report, .. } => LvsState::Done { verdict: report.comparison.result },
        StepResult::Missing => LvsState::Missing,
        StepResult::Error { error } => LvsState::Error { error: error.clone() },
    }
}

fn discrepancies(r: &StepResult) -> Option<Vec<Discrepancy>> {
    match r {
        StepResult::Done { report, .. } => Some(report.comparison.discrepancies()),
        _ => None,
    }
}

/// Transición y delta de `before` a `now` (si los dos tienen resultado).
fn compare(before: Option<&StepResult>, now: &StepResult) -> (Option<Transition>, Option<Delta>) {
    let verdict = |r: &StepResult| state(r).verdict();
    match (before.and_then(verdict), verdict(now), before.and_then(discrepancies), discrepancies(now)) {
        (Some(b), Some(n), Some(db), Some(dn)) => (Transition::between(b, n), Some(Delta::between_results((b, &db), (n, &dn)))),
        _ => (None, None),
    }
}

/// La raíz del working tree del repo de `path`.
fn workdir(repo: &git2::Repository, path: &Path) -> PathBuf {
    repo.workdir().map_or_else(|| path.to_path_buf(), Path::to_path_buf)
}

/// Los resultados por commit, calculados una vez por proceso.
struct Commits<'a> {
    repo: &'a git2::Repository,
    repo_path: &'a Path,
    tools: &'a Tools,
    cache: Cache,
    memo: HashMap<(usize, String), StepResult>,
}

impl Commits<'_> {
    fn at(&mut self, pair_index: usize, pair: &Pair, oid: &str) -> StepResult {
        if let Some(r) = self.memo.get(&(pair_index, oid.to_string())) {
            return r.clone();
        }
        let tree = git2::Oid::from_str(oid).and_then(|o| self.repo.find_commit(o)).and_then(|c| c.tree());
        let result = match tree {
            Ok(tree) => {
                let v = CommitVersion { repo: self.repo, tree };
                let repo_path = self.repo_path;
                result_at(&v, &|| Tree::commit(repo_path, oid), pair, self.tools, &mut self.cache)
            }
            Err(e) => StepResult::Error { error: e.message().to_string() },
        };
        self.memo.insert((pair_index, oid.to_string()), result.clone());
        result
    }
}

/// `riku log --lvs`: el LVS de cada par en cada commit del reporte, respecto
/// de su primer padre. Nunca hace fallar el `log`: sin Netgen, o sin pares,
/// un aviso. Devuelve cuántas veces se corrió Netgen (el resto, de la caché).
pub fn annotate_log(report: &mut LogReport, repo_path: &Path, paths: &[String], level: DetailLevel) -> usize {
    let tools = match tools() {
        Ok(t) => t,
        Err(e) => {
            report.warnings.push(tr!("lvs.unavailable", error = e));
            return 0;
        }
    };
    let repo = match git2::Repository::discover(repo_path) {
        Ok(r) => r,
        Err(e) => {
            report.warnings.push(tr!("lvs.unavailable", error = e.message()));
            return 0;
        }
    };
    let pairs = match project_pairs(&workdir(&repo, repo_path), paths) {
        Ok((p, warnings)) if !p.is_empty() => {
            report.warnings.extend(warnings);
            p
        }
        Ok(_) => {
            report.warnings.push(tr!("lvs.unavailable", error = tr!("lvs.none_found")));
            return 0;
        }
        Err(e) => {
            report.warnings.push(tr!("lvs.unavailable", error = e));
            return 0;
        }
    };
    let mut at = Commits { repo: &repo, repo_path, tools: &tools, cache: Cache::new(), memo: HashMap::new() };
    for c in &mut report.commits {
        for (i, pair) in pairs.iter().enumerate() {
            let now = at.at(i, pair, &c.info.oid);
            let before = c.parents.first().map(|p| at.at(i, pair, p));
            // Un par que no existe ni acá ni en el padre no es de este commit.
            if matches!(now, StepResult::Missing) && before.as_ref().is_none_or(|b| matches!(b, StepResult::Missing)) {
                continue;
            }
            let (transition, delta) = compare(before.as_ref(), &now);
            c.lvs.push(PairLvs {
                schematic: pair.schematic.clone(),
                layout: pair.layout.clone(),
                cell: pair.cell.clone(),
                state: state(&now),
                transition,
                delta,
                discrepancies: if level == DetailLevel::Completo { discrepancies(&now).unwrap_or_default() } else { Vec::new() },
            });
        }
    }
    at.cache.runs
}

/// `riku status --lvs`: por par, el working tree contra `HEAD`. Sin Netgen,
/// cada par queda con el error (el código de salida será 2). Devuelve
/// cuántas veces se corrió Netgen.
pub fn annotate_status(
    report: &mut StatusReport,
    repo_path: &Path,
    paths: &[String],
    level: DetailLevel,
) -> Result<usize, String> {
    let repo = git2::Repository::discover(repo_path).map_err(|e| e.message().to_string())?;
    let root = workdir(&repo, repo_path);
    let (pairs, warnings) = project_pairs(&root, paths)?;
    report.warnings.extend(warnings);
    if pairs.is_empty() {
        report.warnings.push(tr!("lvs.unavailable", error = tr!("lvs.none_found")));
        return Ok(0);
    }
    let tools = match tools() {
        Ok(t) => t,
        Err(e) => {
            report.lvs = pairs
                .iter()
                .map(|p| PairStatusLvs {
                    schematic: p.schematic.clone(),
                    layout: p.layout.clone(),
                    cell: p.cell.clone(),
                    head: LvsState::Error { error: e.clone() },
                    worktree: LvsState::Error { error: e.clone() },
                    unchanged: false,
                    transition: None,
                    delta: None,
                    discrepancies: Vec::new(),
                })
                .collect();
            return Ok(0);
        }
    };
    let head_oid = repo.head().ok().and_then(|h| h.target()).map(|o| o.to_string());
    let mut at = Commits { repo: &repo, repo_path, tools: &tools, cache: Cache::new(), memo: HashMap::new() };
    let disk = DiskVersion { root: root.clone() };
    for (i, pair) in pairs.iter().enumerate() {
        let head = head_oid.as_deref().map_or(StepResult::Missing, |oid| at.at(i, pair, oid));
        let wt = result_at(&disk, &|| Ok(Tree::disk(&root)), pair, &tools, &mut at.cache);
        let (transition, delta) = compare(Some(&head), &wt);
        let unchanged = match (&head, &wt) {
            (StepResult::Done { report: h, .. }, StepResult::Done { report: w, .. }) => h.comparison == w.comparison,
            _ => false,
        };
        report.lvs.push(PairStatusLvs {
            schematic: pair.schematic.clone(),
            layout: pair.layout.clone(),
            cell: pair.cell.clone(),
            head: state(&head),
            worktree: state(&wt),
            unchanged,
            transition,
            delta,
            discrepancies: if level == DetailLevel::Completo { discrepancies(&wt).unwrap_or_default() } else { Vec::new() },
        });
    }
    Ok(at.cache.runs)
}
