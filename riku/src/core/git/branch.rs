use std::collections::HashMap;

use git2::{Reference, Repository};

use crate::core::domain::git_types::{BranchInfo, GitError};
use crate::core::git::helpers::short_oid;
use crate::i18n::tr;

/// Información de la rama actual y su relación con upstream (si existe).
pub(super) fn current_branch(repo: &Repository) -> Result<Option<BranchInfo>, GitError> {
    let head = match repo.head() {
        Ok(h) => h,
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch || e.code() == git2::ErrorCode::NotFound => {
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };

    let head_oid = head.target().ok_or_else(|| GitError::CommitNotFound("HEAD".to_string()))?;
    let head_oid_str = head_oid.to_string();
    let head_short = short_oid(head_oid);

    let name = if head.is_branch() { head.shorthand().unwrap_or("HEAD").to_string() } else { tr!("git.detached") };

    let (upstream, ahead, behind) = if head.is_branch() { upstream_relation(repo, &head)? } else { (None, 0, 0) };

    Ok(Some(BranchInfo { name, head_oid: head_oid_str, head_short, upstream, ahead, behind }))
}

/// Mapa `oid → [ref names]` de las refs locales (ramas + tags) que apuntan
/// a algún commit. Útil para anotar el log con etiquetas.
///
/// Las ramas remotas se incluyen con prefijo `remotes/origin/...` para
/// poder distinguirlas. HEAD aparece como entrada propia si está resuelto.
pub(super) fn refs_by_oid(repo: &Repository) -> Result<HashMap<String, Vec<String>>, GitError> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    // HEAD primero, para que aparezca al inicio en el orden de inserción.
    if let Ok(head) = repo.head() {
        if let Some(oid) = head.target() {
            map.entry(oid.to_string()).or_default().push("HEAD".to_string());
        }
    }

    let refs = repo.references()?;
    for r in refs.flatten() {
        let target = match r.target() {
            Some(o) => o,
            None => continue,
        };
        // Un nombre que no es UTF-8 no se muestra (como antes).
        let name = match r.shorthand() {
            Ok(n) => n.to_string(),
            Err(_) => continue,
        };
        map.entry(target.to_string()).or_default().push(name);
    }
    Ok(map)
}

fn upstream_relation(repo: &Repository, head: &Reference<'_>) -> Result<(Option<String>, usize, usize), GitError> {
    let branch_name = match head.shorthand() {
        Ok(n) => n,
        Err(_) => return Ok((None, 0, 0)),
    };
    let branch = match repo.find_branch(branch_name, git2::BranchType::Local) {
        Ok(b) => b,
        Err(_) => return Ok((None, 0, 0)),
    };
    let upstream = match branch.upstream() {
        Ok(u) => u,
        Err(_) => return Ok((None, 0, 0)),
    };
    let upstream_name = upstream.name().ok().flatten().map(|s| s.to_string());
    let local_oid = head.target().unwrap_or(git2::Oid::ZERO_SHA1);
    let upstream_oid = upstream.get().target().unwrap_or(git2::Oid::ZERO_SHA1);
    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid).unwrap_or((0, 0));
    Ok((upstream_name, ahead, behind))
}
