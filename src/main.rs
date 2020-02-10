use reqwest::{header, Client};
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{self, Command};

fn make_client() -> Client {
    let mut headers = header::HeaderMap::new();

    // Fake a plausible user agent to pass through anti DDOS counter measures.
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_str(
            "Mozilla/5.0 (X11; Linux x86_64; rv:68.0) Gecko Firefox/68.0",
        )
        .unwrap(),
    );

    Client::builder().default_headers(headers).build().unwrap()
}

async fn get_cranelift_version(client: &Client) -> Result<String, Box<dyn std::error::Error>> {
    const URL: &str = "https://crates.io/api/v1/crates/cranelift-codegen";

    let resp = client.get(URL).send().await?.text().await?;

    let object = json::parse(&resp)?;
    let result = &object["crate"]["newest_version"];
    Ok(result.to_string())
}

/// Replace the cranelift version in the Cranelift Cargo.toml file.
fn replace_cranelift_version(repo_path: &str, version: &str) {
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
                format!(
                    r#"cranelift-codegen = {{ version = "{}", default-features = false }}"#,
                    version
                )
            } else if line.starts_with("cranelift-wasm") {
                format!(r#"cranelift-wasm = "{}""#, version)
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

async fn find_last_commit_sha(client: &Client) -> Result<String, Box<dyn std::error::Error>> {
    const URL: &str = "https://api.github.com/repos/bytecodealliance/cranelift/commits/master";

    let resp = client.get(URL).send().await?.text().await?;
    let object = json::parse(&resp)?;
    let result = &object["sha"];

    Ok(result.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();

    let program_name = args.next().unwrap();
    let repo_path = match args.next() {
        Some(path) => path,
        None => {
            println!("missing path to repository");
            println!("usage: {} path/to/repository", program_name);
            process::exit(-1);
        }
    };

    let build_dir = args.next();

    // Set cwd to the repository.
    env::set_current_dir(&repo_path).expect("couldn't set cwd");

    // Make sure the repository doesn't have any changes.
    let output = Command::new("hg")
        .arg("diff")
        .output()
        .expect("couldn't run hg diff");
    if !output.stdout.is_empty() {
        println!("Diff isn't empty! aborting, please make sure the repository is clean before running this script");
        process::exit(-1);
    }

    let client = make_client();

    let version = get_cranelift_version(&client).await?;
    println!("found version {}", version);

    replace_cranelift_version(&repo_path, &version);

    let last_commit = find_last_commit_sha(&client).await?;
    println!("last commit {}", last_commit);

    replace_commit_sha(&repo_path, &last_commit);

    // Commit the change.
    println!("Committing bump patch...");
    let output = Command::new("hg")
        .arg("commit")
        .arg("-m")
        .arg(format!("Bug XXX - Bump Cranelift to {:?}; r?", last_commit))
        .output()
        .expect("couldn't run hg commit");
    if !output.status.success() {
        println!(
            "Couldn't commit: {} {}",
            String::from_utf8(output.stdout)?,
            String::from_utf8(output.stderr)?
        );
        process::exit(-1);
    }

    // Run mach vendor rust.
    println!("Running mach vendor rust...");
    let status = Command::new("./mach")
        .arg("vendor")
        .arg("rust")
        .spawn()
        .expect("couldn't run mach vendor rust")
        .wait()?;
    if !status.success() {
        println!("Error when running mach vendor rust");
        process::exit(-1);
    }

    // Commit the vendor changges.
    println!("Committing vendor patch...");
    let output = Command::new("hg")
        .arg("commit")
        .arg("-m")
        .arg("Bug XXX - Output of mach vendor rust; r?")
        .output()
        .expect("couldn't run hg commit the second time");
    if !output.status.success() {
        println!(
            "Couldn't commit: {} {}",
            String::from_utf8(output.stdout)?,
            String::from_utf8(output.stderr)?
        );
        process::exit(-1);
    }

    if let Some(build_dir) = build_dir {
        // Switch to the build directory, run make, and tests.
        env::set_current_dir(&build_dir).expect("couldn't set cwd to build dir");

        // 8 threads is enough for y'all.
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
            println!("Error when running make",);
            process::exit(-1);
        }

        // TODO run Spidermonkey tests with Cranelift?
    }

    println!("Done, enjoy your day.");
    Ok(())
}
