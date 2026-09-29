use thiserror::Error;

use crate::core::domain::git_types::GitError;
use crate::i18n::tr;

#[derive(Debug, Error)]
pub enum RikuError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("{}", tr!("error.unsupported_format", format = .0))]
    UnsupportedFormat(String),
    #[error("{}", tr!("error.parse", error = .0))]
    Parse(String),
    #[error("{}", tr!("error.render", error = .0))]
    Render(String),
}
