use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct CommandSandbox {
    root: PathBuf,
    enabled: bool,
    allowlist: Vec<String>,
}

impl CommandSandbox {
    pub fn new(root: impl AsRef<Path>, enabled: bool, allowlist: Vec<String>) -> io::Result<Self> {
        Ok(Self {
            root: fs::canonicalize(root)?,
            enabled,
            allowlist,
        })
    }

    pub fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandResult> {
        if !self.enabled {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "terminal access is disabled",
            ));
        }
        if program.contains('/') || !self.allowlist.iter().any(|allowed| allowed == program) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("command '{program}' is not allowlisted"),
            ));
        }
        let started = Instant::now();
        let output = Command::new(program)
            .args(args)
            .current_dir(&self.root)
            .output()?;
        Ok(CommandResult {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            duration: started.elapsed(),
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FileSandbox {
    root: PathBuf,
    read_only: bool,
}

#[allow(dead_code)]
impl FileSandbox {
    pub fn new(root: impl AsRef<Path>, read_only: bool) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        Ok(Self { root, read_only })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn validate_relative(path: &Path) -> io::Result<()> {
        if path.as_os_str().is_empty() || path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path must be relative to the workspace",
            ));
        }
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path traversal is not allowed",
            ));
        }
        Ok(())
    }

    fn existing_path(&self, relative: impl AsRef<Path>) -> io::Result<PathBuf> {
        let relative = relative.as_ref();
        Self::validate_relative(relative)?;
        let path = fs::canonicalize(self.root.join(relative))?;
        if !path.starts_with(&self.root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes the workspace",
            ));
        }
        Ok(path)
    }

    fn writable_path(&self, relative: impl AsRef<Path>) -> io::Result<PathBuf> {
        if self.read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "workspace is read-only",
            ));
        }
        let relative = relative.as_ref();
        Self::validate_relative(relative)?;
        let target = self.root.join(relative);
        let parent = target.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory")
        })?;
        let canonical_parent = fs::canonicalize(parent)?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes the workspace",
            ));
        }
        Ok(target)
    }

    pub fn read_text(&self, relative: impl AsRef<Path>) -> io::Result<String> {
        fs::read_to_string(self.existing_path(relative)?)
    }

    pub fn write_text(&self, relative: impl AsRef<Path>, content: &str) -> io::Result<()> {
        let relative = relative.as_ref();
        Self::validate_relative(relative)?;
        let target = self.root.join(relative);
        let parent = target.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory")
        })?;
        if self.read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "workspace is read-only",
            ));
        }
        fs::create_dir_all(parent)?;
        let canonical_parent = fs::canonicalize(parent)?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path escapes the workspace",
            ));
        }
        fs::write(target, content)
    }

    pub fn list(&self, relative: impl AsRef<Path>) -> io::Result<Vec<String>> {
        let mut entries = fs::read_dir(self.existing_path(relative)?)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        entries.sort();
        Ok(entries)
    }

    pub fn list_workspace(&self) -> io::Result<Vec<String>> {
        let mut entries = fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        entries.sort();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mission-control-sandbox-{id}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        root
    }

    #[test]
    fn reads_writes_and_lists_inside_workspace() {
        let root = root();
        let sandbox = FileSandbox::new(&root, false).unwrap();
        assert!(sandbox.read_text("src/main.rs").unwrap().contains("main"));
        sandbox
            .write_text("src/lib.rs", "pub fn answer() -> u8 { 42 }")
            .unwrap();
        assert_eq!(sandbox.list("src").unwrap(), vec!["lib.rs", "main.rs"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_parent_absolute_and_symlink_escape_paths() {
        let root = root();
        let sandbox = FileSandbox::new(&root, false).unwrap();
        assert!(sandbox.read_text("../secret.txt").is_err());
        assert!(sandbox.read_text("/etc/passwd").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", root.join("outside")).unwrap();
            assert!(sandbox.read_text("outside/secret.txt").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_only_workspace_rejects_writes() {
        let root = root();
        let sandbox = FileSandbox::new(&root, true).unwrap();
        assert!(sandbox.write_text("src/new.rs", "blocked").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_is_denied_when_disabled_or_command_is_not_allowlisted() {
        let root = root();
        let disabled = CommandSandbox::new(&root, false, vec!["git".into()]).unwrap();
        assert!(disabled.run("git", &["status"]).is_err());
        let enabled = CommandSandbox::new(&root, true, vec!["git".into()]).unwrap();
        assert!(enabled.run("sh", &["-c", "echo unsafe"]).is_err());
        assert!(enabled.run("/usr/bin/git", &["status"]).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn allowlisted_command_runs_in_workspace_without_shell() {
        let root = root();
        let sandbox = CommandSandbox::new(&root, true, vec!["rustc".into()]).unwrap();
        let result = sandbox.run("rustc", &["--version"]).unwrap();
        assert_eq!(result.status, 0);
        assert!(result.stdout.starts_with("rustc "));
        fs::remove_dir_all(root).unwrap();
    }
}
