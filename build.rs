fn main() {
    println!("cargo:rerun-if-env-changed=CLASH_LITE_NO_ADMIN");
    // TUN needs admin; debug builds (or CLASH_LITE_NO_ADMIN=1) stay unelevated so they run from a normal shell.
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    if release && std::env::var_os("CLASH_LITE_NO_ADMIN").is_none() {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:level='requireAdministrator'");
    }
}
