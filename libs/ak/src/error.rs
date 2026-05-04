use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    AlreadyExists(PathBuf),
    NotFound(PathBuf),
    NotADirectory(PathBuf),
    UnsafePath(PathBuf),
    Parse(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(
                f,
                "io error: {error}. check filesystem permissions, parent directories, and AK_STORE"
            ),
            Self::AlreadyExists(path) => write!(
                f,
                "path already exists: {}. choose a new path or overwrite intentionally",
                path.display()
            ),
            Self::NotFound(path) => write!(
                f,
                "path not found: {}. check that the path is relative to the store root",
                path.display()
            ),
            Self::NotADirectory(path) => {
                write!(f, "path is not a directory: {}", path.display())
            }
            Self::UnsafePath(path) => write!(
                f,
                "unsafe path blocked: {}. ak paths must stay inside the store and cannot pass through symlinks",
                path.display()
            ),
            Self::Parse(message) => write!(f, "invalid input: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use std::path::PathBuf;

    #[test]
    fn not_found_error_explains_path_interpretation() {
        let error = Error::NotFound(PathBuf::from("/tmp/store/knowledge/missing.md"));
        let rendered = error.to_string();

        assert!(rendered.contains("path not found"));
        assert!(rendered.contains("check that the path is relative to the store root"));
    }

    #[test]
    fn already_exists_error_suggests_intentional_overwrite_or_new_path() {
        let error = Error::AlreadyExists(PathBuf::from("/tmp/store/knowledge/existing.md"));
        let rendered = error.to_string();

        assert!(rendered.contains("path already exists"));
        assert!(rendered.contains("choose a new path or overwrite intentionally"));
    }
}
