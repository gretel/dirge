// Link against the doltlite shared library shipped in vendor/.
// The lib exposes the standard sqlite3_* C API, so the link path
// is identical to system libsqlite3 — we just point at our local
// dylib instead. RPATH is set so the resulting binary can find the
// dylib without LD_LIBRARY_PATH gymnastics.
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_dir = format!(
        "{}/vendor/doltlite-lib-osx-arm64-0.11.2",
        manifest_dir
    );
    println!("cargo:rustc-link-search=native={}", lib_dir);
    println!("cargo:rustc-link-lib=dylib=doltlite");
    // Embed the absolute lib path in the binary's RPATH so it finds
    // libdoltlite.dylib at runtime without env-var setup.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir);
}
