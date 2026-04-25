/// Filtro de paths por glob. Si la lista está vacía, todo coincide.
pub struct PathMatcher {
    patterns: Vec<glob::Pattern>,
}

impl PathMatcher {
    pub fn new(raw: &[String]) -> Self {
        let patterns = raw
            .iter()
            .filter_map(|p| glob::Pattern::new(p).ok())
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
