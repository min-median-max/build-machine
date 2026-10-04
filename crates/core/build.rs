//! The protocol identity of the request and report a controller and a worker
//! exchange: the SHA-256 of every source file of this crate, which defines
//! both documents and how a worker reads a workflow. Any change to them
//! changes the identity, so a worker built from other sources is refused
//! instead of reading a request in its own older shape.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory).unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
    for entry in entries {
        let path = entry.unwrap_or_else(|error| panic!("{}: {error}", directory.display())).path();
        if path.is_dir() {
            sources(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = root.join("src");
    println!("cargo:rerun-if-changed=src");
    let mut found = Vec::new();
    sources(&source, &mut found);
    // Sorted by their path with `/`, so every machine hashes in one order.
    let mut named: Vec<(String, PathBuf)> = found
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(&source).expect("a source under src");
            let name = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            (name, path)
        })
        .collect();
    named.sort();
    let mut hash = Sha256::new();
    for (name, path) in named {
        let data = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update((data.len() as u64).to_le_bytes());
        hash.update(&data);
    }
    println!("cargo:rustc-env=BUILD_MACHINE_PROTOCOL={}", &hex::encode(hash.finalize())[..16]);
}
