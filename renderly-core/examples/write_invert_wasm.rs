//! Write examples/plugins/invert/invert.wasm from the embedded WAT template.
fn main() {
    let bytes = renderly_core::plugins::compile_invert_wasm().expect("compile invert wat");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("examples")
        .join("plugins")
        .join("invert")
        .join("invert.wasm");
    std::fs::write(&path, &bytes).expect("write invert.wasm");
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
}
