use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    let directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
            .join("assets/agents");
    // Watching the directory catches additions/removals as well as prompt edits.
    println!("cargo:rerun-if-changed=assets/agents");
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    files.sort();
    let mut source = String::from("&[\n");
    for path in files {
        let filename = path.file_name().expect("Agent filename").to_string_lossy();
        source.push_str(&format!(
            "    ({filename:?}, include_str!({:?})),\n",
            path.to_string_lossy()
        ));
    }
    source.push_str("]\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(output.join("default_agents.rs"), source)
}
