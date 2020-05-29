use crate::VCS;
use std::path::PathBuf;
use std::process::Command;

pub(crate) struct HG;

impl HG {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl VCS for HG {
    fn is_repo(&self, path: &str) -> bool {
        let mut pathbuf = PathBuf::from(path);
        pathbuf.push(".hg");
        pathbuf.is_dir()
    }

    fn commit(&self, msg: &str) -> Result<(), String> {
        let output = Command::new("hg")
            .arg("commit")
            .arg("-m")
            .arg(msg)
            .output()
            .map_err(|err| format!("couldn't start hg commit: {}", err))?;

        if !output.status.success() {
            let stdout = String::from_utf8(output.stdout).unwrap_or("(stdout unavailable)".into());
            let stderr = String::from_utf8(output.stderr).unwrap_or("(stderr unavailable)".into());
            if stdout.trim() == "nothing changed" {
                Ok(())
            } else {
                Err(format!(
                    "hg commit returned an error: {} {}",
                    stdout, stderr
                ))
            }
        } else {
            Ok(())
        }
    }

    fn has_diff(&self) -> Result<bool, String> {
        let output = Command::new("hg")
            .arg("diff")
            .output()
            .map_err(|err| format!("Could not start hg diff: {}", err))?;
        Ok(!output.stdout.is_empty())
    }
}
