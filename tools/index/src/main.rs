//! The repository's `index.json`, built from what cargo already knows.
//!
//! Every workspace package that sets `package.metadata.vrc-we.index = true`
//! is built for the target the workspace states, copied beside the index,
//! hashed, and listed. Nothing is restated here: the name, version, author
//! and description come from the crate's own manifest and the target from
//! `[workspace.metadata.vrc-we]`, so the index cannot disagree with it.
//!
//! ```text
//! cargo run -p index -- --name community --out dist
//! ```

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The index's format, which the client refuses if it differs.
const FORMAT: u32 = 1;

#[derive(Debug)]
enum Error {
    /// cargo didn't run, or didn't answer with metadata.
    Metadata(String),
    /// The workspace doesn't parse as metadata.
    Json(String),
    /// The workspace doesn't state what a module is built for.
    NoTarget,
    /// A package marked for the index has a metadata table this tool can't
    /// read.
    Marked(String),
    /// A marked package didn't build for the target.
    Build(String, String),
    /// A marked package's module isn't where cargo puts it.
    Missing(PathBuf),
    /// A file couldn't be read, copied or written.
    Io(PathBuf, std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(e) => write!(f, "cargo metadata: {e}"),
            Self::Json(e) => write!(f, "cargo metadata isn't JSON: {e}"),
            Self::NoTarget => write!(
                f,
                "the workspace states no [workspace.metadata.vrc-we] target"
            ),
            Self::Marked(w) => write!(f, "{w}: package.metadata.vrc-we must be an object"),
            Self::Build(p, e) => write!(f, "{p} didn't build for the target: {e}"),
            Self::Missing(p) => {
                write!(f, "{} isn't built: cargo wasm for its package", p.display())
            }
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(summary) => {
            println!("{summary}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("index: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// What one marked package contributes to the document.
struct Marked {
    /// The name it is listed under.
    name: String,
    /// The package cargo builds.
    package: String,
    version: String,
    author: String,
    description: String,
    mature: bool,
    /// Its cdylib's file name, without the extension.
    lib: String,
}

fn run() -> Result<String, Error> {
    let mut name = String::from("community");
    let mut out = PathBuf::from("dist");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--name" => name = args.next().unwrap_or(name),
            "--out" => out = args.next().map(PathBuf::from).unwrap_or(out),
            other => {
                eprintln!("index: ignoring {other:?}");
            }
        }
    }

    let metadata = cargo_metadata()?;
    let target = metadata["metadata"]["vrc-we"]["target"]
        .as_str()
        .ok_or(Error::NoTarget)?
        .to_owned();
    let directory = metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Error::Json("no target_directory".into()))?;
    // Every crate is built through this workspace, whatever directory the
    // tool was run from (an alias at the client repo's root runs it too).
    let workspace = metadata["workspace_root"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Error::Json("no workspace_root".into()))?;
    let release = directory.join(&target).join("release");

    let mut marked: Vec<Marked> = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .ok_or_else(|| Error::Json("no packages".into()))?
    {
        let Some(vrc_we) = package["metadata"].get("vrc-we") else {
            continue;
        };
        let Some(table) = vrc_we.as_object() else {
            return Err(Error::Marked(label(package)));
        };
        if table.get("index").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let package_name = package["name"]
            .as_str()
            .ok_or_else(|| Error::Json("a package without a name".into()))?;
        marked.push(Marked {
            name: table
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(package_name)
                .to_owned(),
            package: package_name.to_owned(),
            version: package["version"]
                .as_str()
                .ok_or_else(|| Error::Json(format!("{package_name} has no version")))?
                .to_owned(),
            author: package["authors"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
            description: package["description"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            mature: table
                .get("mature")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            lib: package["targets"]
                .as_array()
                .and_then(|targets| {
                    targets.iter().find(|t| {
                        t["kind"]
                            .as_array()
                            .is_some_and(|k| k.iter().any(|v| v == "cdylib"))
                    })
                })
                .and_then(|t| t["name"].as_str())
                .ok_or_else(|| Error::Json(format!("{package_name} has no cdylib target")))?
                .to_owned(),
        });
    }
    marked.sort_by(|a, b| a.name.cmp(&b.name));

    std::fs::create_dir_all(&out).map_err(|e| Error::Io(out.clone(), e))?;

    let mut entries = Vec::with_capacity(marked.len());
    for package in &marked {
        build(&workspace, &package.package, &target)?;
        let built = release.join(format!("{}.wasm", package.lib));
        let bytes = std::fs::read(&built).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::Missing(built.clone()),
            _ => Error::Io(built.clone(), e),
        })?;
        let file = format!("{}-{}.wasm", package.name, package.version);
        let placed = out.join(&file);
        std::fs::write(&placed, &bytes).map_err(|e| Error::Io(placed.clone(), e))?;

        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        let sha256: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        entries.push(json!({
            "name": package.name,
            "version": package.version,
            "author": package.author,
            "description": package.description,
            "mature": package.mature,
            "wasm": file,
            "sha256": sha256,
            "size": bytes.len(),
            "min_boundary": vrc_we_sdk::BOUNDARY_VERSION,
        }));
    }

    let document = json!({ "format": FORMAT, "name": name, "extensions": entries });
    let path = out.join("index.json");
    let mut text =
        serde_json::to_string_pretty(&document).map_err(|e| Error::Json(e.to_string()))?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| Error::Io(path.clone(), e))?;
    Ok(format!(
        "{}: {} extension(s) in {}",
        path.display(),
        document["extensions"].as_array().map_or(0, Vec::len),
        out.display()
    ))
}

fn label(package: &Value) -> String {
    package["name"].as_str().unwrap_or("a package").to_owned()
}

/// Builds `package` for `target`; its output is shown if it fails.
fn build(workspace: &Path, package: &str, target: &str) -> Result<(), Error> {
    let out = Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", target, "-p", package])
        .arg("--manifest-path")
        .arg(workspace.join("Cargo.toml"))
        .output()
        .map_err(|e| Error::Build(package.to_owned(), e.to_string()))?;
    if out.status.success() {
        return Ok(());
    }
    Err(Error::Build(
        package.to_owned(),
        String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    ))
}

/// This tool's own workspace: named by its crate's manifest, so the tool
/// reads and builds the same workspace whatever directory it is run from.
fn cargo_metadata() -> Result<Value, Error> {
    let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(own)
        .output()
        .map_err(|e| Error::Metadata(e.to_string()))?;
    if !out.status.success() {
        return Err(Error::Metadata(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| Error::Json(e.to_string()))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    /// The hex the document carries is the published one for `abc`.
    #[test]
    fn the_hash_is_written_as_lowercase_hex() {
        let hash: [u8; 32] = Sha256::digest(b"abc").into();
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
