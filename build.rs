fn main() {
    // rbdc-sqlite (through libsqlite3-sys) and rbdc-turso (through
    // libsql-ffi) both statically bundle SQLite. Because both drivers are
    // always enabled in this crate, the native library contains duplicate
    // SQLite symbols and needs the platform linker to allow them.
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => {
            println!("cargo:rustc-link-arg=/FORCE:MULTIPLE");
        }
        Ok("linux") => {
            println!("cargo:rustc-link-arg=-Wl,--allow-multiple-definition");
        }
        Ok("macos") => {
            println!("cargo:rustc-link-arg=-Wl,-multiply_defined,suppress");
        }
        _ => {}
    }
}
