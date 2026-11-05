fn main() {
    let plugin = std::path::PathBuf::from("resources/model-access").join(format!(
        "{}-{}.bin",
        std::env::var("CARGO_CFG_TARGET_OS").unwrap(),
        std::env::var("CARGO_CFG_TARGET_ARCH").unwrap()
    ));
    println!("cargo:rerun-if-changed={}", plugin.display());
    let plugin = if plugin.is_file() {
        std::fs::canonicalize(plugin).unwrap()
    } else {
        let empty =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("empty-policy.bin");
        std::fs::write(&empty, []).unwrap();
        empty
    };
    println!(
        "cargo:rustc-env=CPA_MODEL_POLICY_PLUGIN={}",
        plugin.display()
    );
    println!("cargo:rerun-if-env-changed=GITCODE_GUI_REPOSITORY");
    println!("cargo:rerun-if-env-changed=GITCODE_CORE_REPOSITORY");
    tauri_build::build()
}
