use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/wpe_hook.c");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android")
        || env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("aarch64")
    {
        return;
    }
    let cc = env::var("CC_aarch64_linux_android")
        .or_else(|_| env::var("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"))
        .unwrap_or_else(|_| "aarch64-linux-android30-clang".into());
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("libskyward_wpe_hook.so");
    let status = Command::new(&cc)
        .args(["-shared", "-nostdlib", "-fPIC", "-O2", "-Wl,-z,max-page-size=4096,--allow-shlib-undefined", "-s", "-o"])
        .arg(&out)
        .arg("src/wpe_hook.c")
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke {cc} for wpe_hook.c: {e}"));
    assert!(status.success(), "{cc} failed compiling wpe_hook.c");
}
