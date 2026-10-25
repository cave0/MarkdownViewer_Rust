//! Generates the table of embedded assets (everything under assets/ except style.css, which
//! is include_str!'d directly), so adding or updating a file needs no code change.
use std::{env, fs, path::{Path, PathBuf}};

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = fs::read_dir(dir).expect("assets dir").flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if rel != "style.css" {
                out.push((rel, p));
            }
        }
    }
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut src = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for (rel, path) in &files {
        src.push_str(&format!("    ({:?}, include_bytes!({:?})),\n", rel, path.to_string_lossy().replace('\\', "/")));
    }
    src.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("assets.rs"), src).unwrap();
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=build.rs");
}
