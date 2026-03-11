use std::path::{Path, PathBuf};
use anyhow::{Result, bail};

/// Walk up from `start` looking for a `.codix` directory.
pub fn find_project_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.canonicalize()?;
    loop {
        if current.join(".codix").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("No codix project found. Run 'codix init' in your project root.");
        }
    }
}

/// Create .codix directory. Error if already exists.
pub fn init_project(dir: &Path) -> Result<PathBuf> {
    let codix_dir = dir.join(".codix");
    if codix_dir.exists() {
        bail!("codix project already initialized in this directory.");
    }
    std::fs::create_dir(&codix_dir)?;
    Ok(codix_dir)
}

/// Path to the SQLite database inside .codix.
pub fn db_path(root: &Path) -> PathBuf {
    root.join(".codix").join("index.db")
}

/// Convert absolute path to path relative to project root.
pub fn relative_to_root(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

/// Convert stored path (relative to root) to display path (relative to CWD).
pub fn display_path(root: &Path, cwd: &Path, stored_path: &str) -> String {
    let abs = root.join(stored_path);
    pathdiff::diff_paths(&abs, cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| stored_path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_find_root_in_current_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".codix")).unwrap();
        let root = find_project_root(tmp.path()).unwrap();
        assert_eq!(root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn test_find_root_in_parent() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".codix")).unwrap();
        let sub = tmp.path().join("src/main/java");
        fs::create_dir_all(&sub).unwrap();
        let root = find_project_root(&sub).unwrap();
        assert_eq!(root, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn test_no_root_found() {
        let tmp = TempDir::new().unwrap();
        let result = find_project_root(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_init_project() {
        let tmp = TempDir::new().unwrap();
        let codix_dir = init_project(tmp.path()).unwrap();
        assert!(codix_dir.exists());
        // Second init should fail
        assert!(init_project(tmp.path()).is_err());
    }

    #[test]
    fn test_relative_to_root() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/src/Foo.java");
        assert_eq!(relative_to_root(&root, &file), "src/Foo.java");
    }
}
