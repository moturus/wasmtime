fn main() {
    println!("cargo:rerun-if-changed=template.ld");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "motor" {
        let script = std::path::Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("template.ld");
        // Place the expandable segment after ordinary sections with both GNU ld and LLD.
        println!(
            "cargo:rustc-link-arg-bin=wasmtime-rt=-Wl,-T,{}",
            script.display()
        );
    }
}
