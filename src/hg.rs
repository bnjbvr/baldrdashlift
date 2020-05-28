use std::process::Command;

pub(crate) fn commit(msg: &str) -> Result<(), String> {
    let output = Command::new("hg")
        .arg("commit")
        .arg("-m")
        .arg(msg)
        .output()
        .map_err(|err| format!("couldn't run hg commit: {}", err))?;

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

pub(crate) fn has_diff() -> Result<bool, String> {
    let output = Command::new("hg")
        .arg("diff")
        .output()
        .map_err(|err| format!("Could not run hg: {}", err))?;
    Ok(!output.stdout.is_empty())
}
