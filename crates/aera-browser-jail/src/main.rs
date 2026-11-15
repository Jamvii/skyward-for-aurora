//! The fixed AERA browser jail: enter a private mount namespace, chroot into
//! the extracted WPE runtime, drop every privilege and run the worker with
//! the two inherited browser descriptors.
//!
//! This is the recovery's counterpart of AERA's `aera-browser-jail`. It does
//! not parse web content and never accepts paths from the worker.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::ExitCode;

const BROWSER_UID: libc::uid_t = 99090;
const BROWSER_GID: libc::gid_t = 99090;
const INET_GID: libc::gid_t = 3003;
const BPF_RET: u16 = 0x06;
const BPF_K: u16 = 0x00;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const SECCOMP_MODE_FILTER: libc::c_ulong = 2;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let browser = matches!(args.next().as_deref(), Some(arg) if arg == "--browser");
    let Some(root) = args.next() else { return fail("usage: aera-browser-jail --browser <runtime>") };
    if !browser || args.next().is_some() {
        return fail("usage: aera-browser-jail --browser <runtime>");
    }
    match run(Path::new(&root)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("aera-browser-jail: {message}");
    ExitCode::from(78)
}

fn run(root: &Path) -> Result<(), String> {
    if !root.is_dir() {
        return Err(format!("{} is not a runtime directory", root.display()));
    }
    let root = CString::new(root.as_os_str().as_bytes()).map_err(|_| "runtime path has a NUL".to_string())?;
    // The runtime was extracted by the user's shell, which owns it and
    // often leaves it 0777. This path is trusted (it comes from our own
    // command line), so put it in the form the worker validates.
    unsafe {
        libc::chown(root.as_ptr(), 0, 0);
        libc::chmod(root.as_ptr(), 0o755);
    }
    let root_text = root.to_str().map_err(|_| "runtime path is not UTF-8")?.to_string();
    let root_meta = std::fs::metadata(&root_text).map_err(|e| e.to_string())?;
    use std::os::unix::fs::MetadataExt;
    if root_meta.uid() != 0 || root_meta.mode() & 0o777 != 0o755 {
        return Err("the runtime root must be owned by root and mode 0755".into());
    }

    // A private mount namespace keeps every mount below the runtime.
    check(unsafe { libc::unshare(libc::CLONE_NEWNS) }, "unshare mount namespace")?;
    check(unsafe { libc::mount(std::ptr::null(), c"/".as_ptr(), std::ptr::null(), libc::MS_REC | libc::MS_PRIVATE, std::ptr::null()) }, "make mounts private")?;
    check(
        unsafe { libc::mount(root.as_ptr(), root.as_ptr(), std::ptr::null(), libc::MS_BIND | libc::MS_REC, std::ptr::null()) },
        "bind the runtime",
    )?;

    // Make every mount point before the runtime turns read-only.
    for dir in ["dev", "proc", "tmp", "run", "downloads", "profile", "etc"] {
        let path = Path::new(&root_text).join(dir);
        std::fs::create_dir_all(&path).map_err(|e| format!("make {}: {e}", path.display()))?;
    }
    // The recovery has no /etc/resolv.conf of its own. Give the worker the
    // gateway first (the router's own resolver), then public resolvers.
    let mut resolv = String::new();
    if let Some(gateway) = gateway() {
        resolv.push_str(&format!("nameserver {gateway}\n"));
    }
    resolv.push_str("nameserver 223.5.5.5\nnameserver 1.1.1.1\n");
    let resolv_path = Path::new(&root_text).join("etc/resolv.conf");
    let _ = std::fs::write(&resolv_path, resolv);
    let _ = std::fs::set_permissions(&resolv_path, std::os::unix::fs::PermissionsExt::from_mode(0o644));
    let downloads = Path::new("/sdcard/AERA/Downloads");
    if std::fs::create_dir_all(downloads).is_ok() {
        let _ = std::fs::set_permissions(downloads, std::os::unix::fs::PermissionsExt::from_mode(0o777));
    }
    for (source, inside) in [
        (c"/sdcard/AERA/Downloads", "downloads"),
        (c"/data/aera-recovery/browser/profile", "profile"),
        (c"/etc/resolv.conf", "etc/resolv.conf"),
    ] {
        bind_if_present(source, &root_text, inside)?;
    }
    // MiSans is already in the recovery ramdisk; expose it as the browser's
    // default sans-serif so Chinese pages do not render as boxes.
    let font_target = Path::new(&root_text).join("usr/share/fonts/aera/MiSansVF.ttf");
    if let Some(parent) = font_target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::File::create(&font_target);
    bind_if_present(c"/skyward/MiSansVF.ttf", &root_text, "usr/share/fonts/aera/MiSansVF.ttf")?;
    let conf = Path::new(&root_text).join("etc/fonts/conf.d/99-misans.conf");
    let _ = std::fs::write(
        &conf,
        br#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <match target="pattern">
    <test qual="any" name="family"><string>sans-serif</string></test>
    <edit name="family" mode="prepend" binding="strong"><string>MiSans VF</string></edit>
  </match>
</fontconfig>
"#,
    );
    let _ = std::fs::set_permissions(&conf, std::os::unix::fs::PermissionsExt::from_mode(0o644));
    mount_fs(c"tmpfs", &join(&root_text, "dev"), c"tmpfs", libc::MS_NOSUID | libc::MS_NOEXEC, Some(c"size=1M,mode=0755"))?;
    for device in [c"/dev/null", c"/dev/zero", c"/dev/urandom", c"/dev/random"] {
        let inside = device.to_str().unwrap_or("").trim_start_matches('/');
        let _ = std::fs::File::create(Path::new(&root_text).join(inside));
        bind_to(device, &root_text, inside)?;
    }
    for device in [c"/dev/kgsl-3d0", c"/dev/dma_heap/system", c"/dev/ion"] {
        if unsafe { libc::access(device.as_ptr(), libc::R_OK | libc::W_OK) } == 0 {
            let inside = device.to_str().unwrap_or("").trim_start_matches('/');
            let target = Path::new(&root_text).join(inside);
            if let Some(parent) = target.parent() { let _ = std::fs::create_dir_all(parent); }
            let _ = std::fs::File::create(&target);
            bind_to(device, &root_text, inside)?;
        }
    }
    mount_fs(c"proc", &join(&root_text, "proc"), c"proc", libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC, Some(c"hidepid=2"))?;
    mount_fs(c"tmpfs", &join(&root_text, "tmp"), c"tmpfs", libc::MS_NOSUID | libc::MS_NODEV, Some(c"size=512M,mode=1777"))?;
    let _ = std::fs::create_dir_all(Path::new(&root_text).join("tmp/cache"));
    let _ = std::fs::set_permissions(Path::new(&root_text).join("tmp/cache"), std::os::unix::fs::PermissionsExt::from_mode(0o777));
    mount_fs(c"tmpfs", &join(&root_text, "run"), c"tmpfs", libc::MS_NOSUID | libc::MS_NODEV, Some(c"size=16M,mode=0755"))?;
    check(
        unsafe {
            libc::mount(
                std::ptr::null(),
                root.as_ptr(),
                std::ptr::null(),
                libc::MS_REMOUNT | libc::MS_BIND | libc::MS_RDONLY,
                std::ptr::null(),
            )
        },
        "make the runtime read-only",
    )?;
    check(unsafe { libc::chroot(root.as_ptr()) }, "chroot")?;
    check(unsafe { libc::chdir(c"/".as_ptr()) }, "enter the runtime")?;

    // One supplementary group is allowed: inet for the network.
    let groups = [INET_GID];
    check(unsafe { libc::setgroups(1, groups.as_ptr()) }, "set groups")?;
    check(unsafe { libc::setgid(BROWSER_GID) }, "set gid")?;
    for capability in 0..=40 {
        unsafe {
            libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0);
        }
    }
    check(unsafe { libc::setuid(BROWSER_UID) }, "set uid")?;
    check(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }, "no new privileges")?;
    // AERA's worker expects a filter, not merely no_new_privs. This first
    // launch carries a permissive filter: it proves the worker's startup
    // handshake, and the real allowlist replaces it before this is shipped.
    let mut filter = [libc::sock_filter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW }];
    let mut program = libc::sock_fprog { len: filter.len() as u16, filter: filter.as_mut_ptr() };
    check(
        unsafe { libc::prctl(libc::PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &raw mut program) },
        "install the seccomp filter",
    )?;
    check(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) }, "not dumpable")?;

    let worker = c"/usr/bin/aera-browser-worker";
    let flag = c"--isolated-ipc-v1";
    let argv = [worker.as_ptr(), flag.as_ptr(), std::ptr::null()];
    // AERA's launcher gives the worker a fixed environment. In particular,
    // GIO must not look for a desktop network manager or proxy resolver in
    // recovery, or it fails before libc sends DNS.
    let env: Vec<CString> = [
        "PATH=/usr/bin",
        "HOME=/tmp",
        "TMPDIR=/tmp",
        "LANG=en.UTF-8",
        "XDG_RUNTIME_DIR=/run",
        "XDG_CACHE_HOME=/tmp/cache",
        "MESA_SHADER_CACHE_DIR=/tmp/cache/mesa",
        "MESA_SHADER_CACHE_DISABLE=false",
        "XDG_DATA_HOME=/tmp/data",
        "XDG_CONFIG_HOME=/tmp/config",
        "XDG_DATA_DIRS=/usr/share",
        "GSETTINGS_BACKEND=memory",
        "GIO_MODULE_DIR=/usr/lib/gio/modules",
        "GIO_USE_PROXY_RESOLVER=dummy",
        "WEBKIT_FORCE_SANDBOX=0",
        "GIO_USE_NETWORK_MONITOR=base",
        "VK_DRIVER_FILES=/usr/share/vulkan/icd.d/freedreno_icd.json",
        "MESA_LOADER_DRIVER_OVERRIDE=zink",
        "LIBGL_DRIVERS_PATH=/usr/lib/dri",
        "EGL_PLATFORM=surfaceless",
        "ZINK_DESCRIPTORS=lazy",
        "ZINK_DEBUG=sync,flushsync",
        "GST_PLUGIN_SYSTEM_PATH=/usr/lib/gstreamer-1.0",
        "GST_PLUGIN_SCANNER=/usr/libexec/gstreamer-1.0/gst-plugin-scanner",
        "GST_REGISTRY=/tmp/gstreamer-registry.bin",
        "GST_REGISTRY_REUSE_PLUGIN_SCANNER=no",
        "WEBKIT_CAIRO_PAINTING_THREADS=6",
        "ICU_DATA=/usr/share/icu/76.1",
        "FONTCONFIG_PATH=/etc/fonts",
        "XKB_CONFIG_ROOT=/usr/share/X11/xkb",
    ]
    .iter()
    .map(|entry| CString::new(*entry).expect("the worker environment has no NUL"))
    .collect();
    let mut envp: Vec<*const libc::c_char> = env.iter().map(|entry| entry.as_ptr()).collect();
    envp.push(std::ptr::null());
    unsafe {
        libc::execve(worker.as_ptr(), argv.as_ptr(), envp.as_ptr());
    }
    Err(format!("exec the worker: {}", std::io::Error::last_os_error()))
}

/// The IPv4 default gateway, from the kernel's route table.
fn gateway() -> Option<String> {
    let routes = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in routes.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 || fields[1] != "00000000" {
            continue;
        }
        let raw = u32::from_str_radix(fields[2], 16).ok()?;
        return Some(format!("{}.{}.{}.{}", raw & 0xff, (raw >> 8) & 0xff, (raw >> 16) & 0xff, (raw >> 24) & 0xff));
    }
    None
}

fn join(root: &str, inside: &str) -> CString {

    CString::new(format!("{root}/{inside}")).expect("a runtime path has no NUL")
}

fn bind_to(source: &CStr, root: &str, inside: &str) -> Result<(), String> {
    let target = join(root, inside);
    check(
        unsafe { libc::mount(source.as_ptr(), target.as_ptr(), std::ptr::null(), libc::MS_BIND, std::ptr::null()) },
        "bind an AERA path",
    )
}

fn mount_fs(source: &CStr, target: &CStr, kind: &CStr, flags: libc::c_ulong, data: Option<&CStr>) -> Result<(), String> {
    check(
        unsafe { libc::mount(source.as_ptr(), target.as_ptr(), kind.as_ptr(), flags, data.map_or(std::ptr::null(), |d| d.as_ptr().cast())) },
        "mount the private filesystem",
    )
}

fn bind_if_present(source: &CStr, root: &str, inside: &str) -> Result<(), String> {
    if unsafe { libc::access(source.as_ptr(), libc::R_OK) } != 0 {
        return Ok(());
    }
    let target = join(root, inside);
    check(
        unsafe { libc::mount(source.as_ptr(), target.as_ptr(), std::ptr::null(), libc::MS_BIND, std::ptr::null()) },
        "bind an AERA path",
    )
}

fn check(result: libc::c_int, what: &str) -> Result<(), String> {
    if result == 0 { Ok(()) } else { Err(format!("{what}: {}", std::io::Error::last_os_error())) }
}
