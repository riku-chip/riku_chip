/// Filtro de paths por glob. Si la lista está vacía, todo coincide.
pub struct PathMatcher {
    patterns: Vec<glob::Pattern>,
}

impl PathMatcher {
    /// Un patrón que no es un glob válido (`amp[1.sch`) se busca tal cual:
    /// nunca se descarta, así un filtro mal escrito no deja pasar todo. La
    /// CLI los rechaza antes con [`check_glob`].
    pub fn new(raw: &[String]) -> Self {
        let patterns = raw
            .iter()
            .map(|p| glob::Pattern::new(p).unwrap_or_else(|_| glob::Pattern::new(&glob::Pattern::escape(p)).expect("escapado")))
            .collect();
        Self { patterns }
    }

    pub fn matches(&self, path: &str) -> bool {
        if self.patterns.is_empty() {
            return true;
        }
        self.patterns.iter().any(|p| p.matches(path))
    }
}

/// Valida un glob de `--paths` (para `clap`): el error dice qué está mal.
pub fn check_glob(p: &str) -> Result<String, String> {
    glob::Pattern::new(p).map(|_| p.to_string()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_glob_is_matched_literally_instead_of_dropped() {
        let m = PathMatcher::new(&["amp[1.sch".to_string()]);
        assert!(m.matches("amp[1.sch"));
        assert!(!m.matches("otro.sch"), "un filtro inválido no deja pasar todo");
        assert!(PathMatcher::new(&[]).matches("otro.sch"));
        assert!(check_glob("amp[1.sch").is_err());
        assert_eq!(check_glob("amp_*.sch").as_deref(), Ok("amp_*.sch"));
    }
}
