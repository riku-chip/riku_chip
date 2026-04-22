use std::path::PathBuf;

pub struct LaunchArgs {
    pub file: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub commit_a: Option<String>,
    pub commit_b: Option<String>,
}

pub fn parse_args() -> LaunchArgs {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let mut repo = None;
    let mut commit_a = None;
    let mut commit_b = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo"     => repo     = args.next().map(PathBuf::from),
            "--commit-a" => commit_a = args.next(),
            "--commit-b" => commit_b = args.next(),
            _            => file     = Some(PathBuf::from(arg)),
        }
    }

    LaunchArgs { file, repo, commit_a, commit_b }
}
