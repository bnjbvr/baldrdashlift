use env::Args;
use std::env;
use std::fs::{canonicalize, File};
use std::io::{Read, Write};
use std::path::Path;
use std::{
    error::Error,
    fmt,
    process::{self, Command},
};

mod git;
mod hg;

trait VCS {
    fn is_repo(&self, path: &str) -> bool;
    fn commit(&self, msg: &str) -> Result<(), String>;
    fn has_diff(&self) -> Result<bool, String>;
}

fn get_vcs_for_repo(path: &str) -> Result<Box<dyn VCS>, Box<dyn Error>> {
    let h = hg::HG::new();
    let g = git::Git::new();
    if h.is_repo(path) {
        Ok(Box::new(h))
    } else if g.is_repo(path) {
        Ok(Box::new(g))
    } else {
        Err(format!("Not a git or Mercurial repository: {}", path).into())
    }
}

const CRANELIFT_JS_SHELL_ARGS: &'static str =
    "--no-wasm-simd --shared-memory=off --wasm-compiler=cranelift";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();

    let _ = args.next().unwrap();

    let command = match args.next() {
        Some(command) => command,
        None => show_usage(),
    };

    match command.as_str() {
        "build" => run_build(args).await,
        "bump" => run_bump(args).await,
        "local" => run_local(args).await,
        "test" => run_test(args).await,
        _ => show_usage(),
    }
}

struct SimpleError(&'static str);

impl fmt::Debug for SimpleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for SimpleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for SimpleError {}

fn make_http_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();

    // Fake a plausible user agent to pass through anti DDOS counter measures.
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_str(
            "Mozilla/5.0 (X11; Linux x86_64; rv:68.0) Gecko Firefox/68.0",
        )
        .unwrap(),
    );

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

async fn get_cranelift_version(
    client: &reqwest::Client,
) -> Result<String, Box<dyn std::error::Error>> {
    const URL: &str = "https://crates.io/api/v1/crates/cranelift-codegen";

    let resp = client.get(URL).send().await?.text().await?;

    let object = json::parse(&resp)?;
    let result = &object["crate"]["newest_version"];
    Ok(result.to_string())
}

enum VersionSpec {
    Fixed(String),
    Path(String),
}

/// Replace the cranelift version in the Cranelift Cargo.toml file.
fn replace_cranelift_version(repo_path: &str, version: VersionSpec) {
    println!("Replacing Cranelift version in its cargo file...");
    let cranelift_cargo_path = Path::new(&repo_path)
        .join("js")
        .join("src")
        .join("wasm")
        .join("cranelift")
        .join("Cargo.toml");

    let mut file = File::open(&cranelift_cargo_path).expect("couldn't open Cranelift cargo file");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("couldn't read Cranelift cargo file content");

    let content_lines = content.split("\n");

    let new_content = content_lines
        .map(|line| {
            if line.starts_with("cranelift-codegen =") {
                let replacement = match &version {
                    VersionSpec::Fixed(version_number) => {
                        format!("version = \"{}\"", version_number)
                    }
                    VersionSpec::Path(path) => format!("path = \"{}codegen\"", path),
                };
                format!(
                    r#"cranelift-codegen = {{ {}, default-features = false }}"#,
                    replacement
                )
            } else if line.starts_with("cranelift-wasm") {
                let replacement = match &version {
                    VersionSpec::Fixed(version_number) => {
                        format!("version = \"{}\"", version_number)
                    }
                    VersionSpec::Path(path) => format!("path = \"{}wasm\"", path),
                };
                format!(r#"cranelift-wasm = {{ {} }}"#, replacement)
            } else {
                line.into()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut file = File::create(&cranelift_cargo_path)
        .expect("couldn't open Cranelift cargo file in write mode");
    file.write_all(new_content.as_bytes())
        .expect("couldn't write new Cranelift cargo content");
    println!("Done!");
}

/// Replace the cranelift version in the top-level Cargo.toml file.
fn replace_commit_sha(repo_path: &str, sha: &str) {
    println!("Replacing Cranelift commit hash in the top-level cargo file...");
    let toplevel_cargo_path = Path::new(&repo_path).join("Cargo.toml");

    let mut file = File::open(&toplevel_cargo_path).expect("couldn't open Cranelift cargo file");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("couldn't read Cranelift cargo file content");

    let content_lines = content.split("\n");

    // Small state machine: when we see the patch line, we know we need to replace the line in 2
    // lines. Very adhoc, but, oh well.
    let mut replace_in = None;
    let new_content = content_lines
        .map(|line| {
            replace_in = match replace_in {
                Some(x) if x > 0 => Some(x - 1),
                _ => None,
            };
            let ret = if let Some(0) = &replace_in {
                format!(r#"rev = "{}""#, sha)
            } else {
                line.into()
            };
            if line.starts_with("[patch.crates-io.cranelift-") {
                replace_in = Some(2);
            }
            ret
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut file = File::create(&toplevel_cargo_path)
        .expect("couldn't open Cranelift cargo file in write mode");
    file.write_all(new_content.as_bytes())
        .expect("couldn't write new Cranelift cargo content");
    println!("Done!");
}

async fn find_last_commit_sha(
    client: &reqwest::Client,
) -> Result<String, Box<dyn std::error::Error>> {
    const URL: &str = "https://api.github.com/repos/bytecodealliance/wasmtime/commits/HEAD";

    let resp = client.get(URL).send().await?.text().await?;
    let object = json::parse(&resp)?;
    let result = &object["sha"];

    Ok(result.to_string())
}

fn mach_vendor_rust(allow_large: bool) -> Result<(), Box<dyn Error>> {
    println!("Running mach vendor rust...");
    let mut command = Command::new("./mach");
    command.arg("vendor").arg("rust");
    if allow_large {
        command.arg("--build-peers-said-large-imports-were-ok");
    }
    let status = command
        .spawn()
        .expect("couldn't run mach vendor rust")
        .wait()?;
    if !status.success() {
        return Err(Box::new(SimpleError("Error when running mach vendor rust")));
    }
    Ok(())
}

fn check_gecko_repo(repo_path: &str) -> Result<Box<dyn VCS>, Box<dyn Error>> {
    // Set cwd to the repository.
    env::set_current_dir(repo_path)?;

    let repo = get_vcs_for_repo(repo_path)?;

    // Make sure the repository doesn't have any changes.
    if repo.has_diff()? {
        return Err(Box::new(SimpleError("Diff isn't empty! aborting, please make sure the repository is clean before running this script".into())));
    }

    Ok(repo)
}

/// Canonicalizes a relative/absolute dir path into an absolute path with a trailing slash at the
/// end.
fn canonicalize_dir(s: String) -> String {
    // Canonicalize the path.
    let pathbuf = canonicalize(&s).expect("Could not canonicalize path");
    let mut s = pathbuf.to_str().expect("Path is not UTF-8").to_string();

    // Add the trailing slash if it's not there yet.
    if s.ends_with("/") {
        s
    } else {
        s += &"/";
        s
    }
}

fn get_repo_arg(args: &mut Args) -> String {
    match args.next() {
        Some(path) => canonicalize_dir(path),
        None => {
            println!("Missing repository path.");
            show_usage()
        }
    }
}

async fn run_bump(mut args: Args) -> Result<(), Box<dyn Error>> {
    let repo_path = &get_repo_arg(&mut args);
    let repo = check_gecko_repo(repo_path)?;

    let large_imports = if let Some(arg) = args.next() {
        match arg.as_str() {
            "--allow-large" | "-a" => true,
            _ => return Err(format!("unknown bump option: {}", arg).into()),
        }
    } else {
        false
    };

    let http_client = make_http_client();

    let version = get_cranelift_version(&http_client).await?;
    println!("found version {}", version);
    replace_cranelift_version(&repo_path, VersionSpec::Fixed(version));

    let last_commit = find_last_commit_sha(&http_client).await?;
    println!("last commit {}", last_commit);
    replace_commit_sha(&repo_path, &last_commit);

    // Commit the change.
    println!("Committing bump patch...");
    repo.commit(&format!("Bug XXX - Bump Cranelift to {}; r?", last_commit))?;

    // Run mach vendor rust.
    mach_vendor_rust(large_imports)?;

    // Commit the vendor changges.
    println!("Committing vendor patch...");
    repo.commit("Bug XXX - Output of mach vendor rust; r?")?;

    println!("Done, enjoy your day.");
    Ok(())
}

async fn run_build(mut args: Args) -> Result<(), Box<dyn Error>> {
    let build_dir = match args.next() {
        Some(build_dir) => build_dir,
        None => {
            return Err(Box::new(SimpleError(
                "usage of `build`: build PATH_TO_BUILD_DIR",
            )))
        }
    };
    let build_dir = canonicalize_dir(build_dir);

    // Switch to the build directory, run make, and tests.
    env::set_current_dir(&build_dir).expect("couldn't set cwd to build dir");

    // As many threads as there are cpus, or 8 by default.
    let nproc = Command::new("nproc").output();
    let nproc = match nproc {
        Ok(output) => {
            let mut string = String::from_utf8(output.stdout)?;
            string.retain(|c| !c.is_whitespace());
            string.parse::<u32>()?
        }
        Err(_) => 8,
    };

    println!("Running make...");
    let status = Command::new("make")
        .arg(format!("-sj{}", nproc))
        .spawn()
        .expect("couldn't run make")
        .wait()?;
    if !status.success() {
        return Err(Box::new(SimpleError("Error when running make")));
    }

    Ok(())
}

async fn run_local(mut args: Args) -> Result<(), Box<dyn Error>> {
    // Read arguments: GECKO_PATH WASMTIME_PATH
    let repo_path = get_repo_arg(&mut args);

    let wasmtime_path = match args.next() {
        Some(path) => path,
        None => {
            return Err(Box::new(SimpleError(
                "usage of `local`: local GECKO_REPO_PATH WASMTIME_REPO_PATH",
            )));
        }
    };
    let cranelift_path = canonicalize_dir(wasmtime_path) + &"cranelift/";

    let repo = check_gecko_repo(&repo_path)?;

    // Replace the version of Cranelift in the Cargo.toml file.
    replace_cranelift_version(&repo_path, VersionSpec::Path(cranelift_path));

    // Commit the change.
    println!("Committing bump patch...");
    repo.commit("No bug - do not check in - use local Cranelift")?;

    // Run mach vendor rust.
    mach_vendor_rust(false)?;

    // Commit the vendor changges.
    println!("Committing vendor patch...");
    repo.commit("No bug - do not check in - result of mach vendor rust")?;

    println!("Done, enjoy your day.");

    Ok(())
}

async fn run_test(mut args: Args) -> Result<(), Box<dyn Error>> {
    let repo_path = get_repo_arg(&mut args);

    let build_path = canonicalize_dir(match args.next() {
        Some(path) => path,
        None => {
            return Err(Box::new(SimpleError(
                "usage of `test`: test GECKO_DIR BUILD_DIR",
            )))
        }
    });
    let path_to_shell = build_path + "dist/bin/js";

    let path_to_jit_tests = Path::join(Path::new(&repo_path), "js/src/jit-test/jit_test.py");

    let shell_args = format!("--args \"{}\"", CRANELIFT_JS_SHELL_ARGS);

    // Defaults to running the wasm test cases.
    let which_tests = match args.next() {
        Some(prefix) => prefix,
        None => "wasm".to_string(),
    };

    println!("Running tests...");
    let status = Command::new(path_to_jit_tests)
        .arg(path_to_shell)
        .arg(shell_args)
        .arg(which_tests)
        .spawn()
        .expect("couldn't run tests")
        .wait()?;

    if !status.success() {
        Err(Box::new(SimpleError("Test failures!")))
    } else {
        Ok(())
    }
}

fn show_usage() -> ! {
    println!("usage: PROGRAM COMMAND");
    println!("  where COMMAND is one of:");
    println!(
        "  bump GECKO_DIR                   bump to the latest available version of Cranelift in tree"
    );
    println!("  build BUILD_DIR                  run make in the build directory");
    println!(
        "  local GECKO_DIR WASMTIME_DIR     use the local version of Cranelift in this Gecko tree"
    );
    println!("  test GECKO_DIR BUILD_DIR PREFIX  run wasm tests with Cranelift");
    process::exit(-1);
}
