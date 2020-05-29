use crate::VCS;
use std::path::PathBuf;
use std::process::Command;

pub(crate) struct Git();

impl Git {
    pub(crate) fn new() -> Git {
        Git()
    }
}

impl VCS for Git {
    fn is_repo(&self, path: &str) -> bool {
        let mut pathbuf = PathBuf::from(path);
        pathbuf.push(".git");
        pathbuf.is_dir()
    }

    fn commit(&self, msg: &str) -> Result<(), String> {
        let output = Command::new("git")
            .arg("commit")
            .arg("-am")
            .arg(msg)
            .output()
            .map_err(|err| format!("couldn't run git commit: {}", err))?;

        if !output.status.success() {
            let stdout = String::from_utf8(output.stdout).unwrap_or("(stdout unavailable)".into());
            let stderr = String::from_utf8(output.stderr).unwrap_or("(stderr unavailable)".into());
            if stdout.trim() == "nothing changed" {
                Ok(())
            } else {
                Err(format!("Couldn't commit: {} {}", stdout, stderr))
            }
        } else {
            Ok(())
        }
    }

    fn has_diff(&self) -> Result<bool, String> {
        let output = Command::new("git")
            .arg("diff")
            .output()
            .map_err(|err| format!("Could not run hg: {}", err))?;
        Ok(!output.stdout.is_empty())
    }
}
