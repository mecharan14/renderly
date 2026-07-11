//! One-shot helper: regenerate `examples/plugins/gain/gain.wasm`.
fn main() {
    let bytes = renderly_core::compile_gain_wasm().expect("compile gain wat");
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins/gain/gain.wasm");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out, &bytes).expect("write gain.wasm");
    println!("wrote {} bytes to {}", bytes.len(), out.display());
}
