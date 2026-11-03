fn main() {
    // Rebuild Windows resources when generated brand icons change.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
