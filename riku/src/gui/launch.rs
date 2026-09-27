use std::path::PathBuf;

pub struct LaunchArgs {
    pub file: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub commit_a: Option<String>,
    pub commit_b: Option<String>,
    /// Sub-vista a abrir (celda GDS). `None` = la que elija el backend.
    pub cell: Option<String>,
    /// Señales calculadas para la vista de formas de onda (`--expr`).
    #[cfg_attr(not(feature = "spice"), allow(dead_code))]
    pub exprs: Vec<String>,
}

/// Argumentos del visor (sin el nombre del programa).
pub fn parse_args(mut args: impl Iterator<Item = String>) -> LaunchArgs {
    let mut file = None;
    let mut repo = None;
    let mut commit_a = None;
    let mut commit_b = None;
    let mut cell = None;
    let mut exprs = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo"     => repo     = args.next().map(PathBuf::from),
            "--commit-a" => commit_a = args.next(),
            "--commit-b" => commit_b = args.next(),
            "--cell"     => cell     = args.next(),
            "--expr"     => exprs.extend(args.next()),
            _            => file     = Some(PathBuf::from(arg)),
        }
    }

    LaunchArgs { file, repo, commit_a, commit_b, cell, exprs }
}
