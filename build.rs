//! Platform build shims for GPUI.
//!
//! - **Windows (MSVC):**
//!   - Embeds a side-by-side application manifest requesting
//!     `Microsoft.Windows.Common-Controls` `6.0.0.0` (so `TaskDialogIndirect`,
//!     imported by `gpui` / `rfd` from `comctl32.dll`, resolves at launch instead
//!     of failing with `0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND)`), declaring
//!     Windows 10/11 compatibility and `asInvoker` trust info, AND declaring
//!     `PerMonitorV2` DPI awareness so Windows renders crisp native resolution
//!     text without DWM bitmap scaling blur.
//!   - Compiles and embeds a Win32 `.res` resource containing the VanguardCV
//!     application icon (`assets/branding/vanguard-cv.ico`) and `VS_VERSION_INFO`
//!     metadata derived from `CARGO_PKG_VERSION`.
//! - **Linux (Wayland):** `xkbcommon` links `libxkbcommon` and
//!   `libxkbcommon-x11` by their unversioned names, which are normally only
//!   installed by `-dev` packages. When only runtime `*.so.0` files exist,
//!   we expose symlinks in `OUT_DIR` and add it to the native link search path.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `(unversioned link name, runtime soname)`.
const LIBS: [(&str, &str); 2] = [
    ("libxkbcommon.so", "libxkbcommon.so.0"),
    ("libxkbcommon-x11.so", "libxkbcommon-x11.so.0"),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/branding/vanguard-cv.ico");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        configure_windows_resources();
        return;
    }

    if target_os != "linux" {
        return;
    }
    let host = env::var("HOST").unwrap_or_default();
    let target = env::var("TARGET").unwrap_or_default();
    if host != target {
        // The runtime libraries we can symlink belong to the build host.
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));
    let link_dir = out_dir.join("native-libs");
    let search_dirs = candidate_dirs();
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut linked_any = false;

    for (unversioned, versioned) in LIBS {
        #[cfg_attr(not(unix), allow(unused_variables))]
        let Some(source) = search_dirs
            .iter()
            .map(|dir| dir.join(versioned))
            .find(|path| path.exists())
        else {
            continue;
        };

        if fs::create_dir_all(&link_dir).is_err() {
            continue;
        }

        let link = link_dir.join(unversioned);
        if link.exists() {
            let _ = fs::remove_file(&link);
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            if symlink(&source, &link).is_ok() {
                linked_any = true;
            }
        }
    }

    if linked_any {
        println!("cargo:rustc-link-search=native={}", link_dir.display());
    }
}

fn parse_quad_version() -> (u16, u16, u16, u16) {
    let major = env::var("CARGO_PKG_VERSION_MAJOR")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(0);
    let minor = env::var("CARGO_PKG_VERSION_MINOR")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(0);
    let patch = env::var("CARGO_PKG_VERSION_PATCH")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(1);
    (major, minor, patch, 0)
}

fn configure_windows_resources() {
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    let (maj, min, pat, bld) = parse_quad_version();
    let ver_dot = format!("{maj}.{min}.{pat}.{bld}");
    let ver_comma = format!("{maj},{min},{pat},{bld}");
    let pkg_ver = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.1".to_string());

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR set by cargo"));

    // 1. Embed Common Controls v6 + Windows 10/11 compatibility + PerMonitorV2 DPI manifest.
    let manifest_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
    type="win32"
    name="VanguardCV"
    version="{ver_dot}"
    processorArchitecture="*"
  />
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 and Windows 11 -->
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#
    );

    let manifest_path = out_dir.join("vanguard-cv.manifest");
    fs::write(&manifest_path, manifest_xml)
        .expect("failed to write Windows Common Controls v6 manifest");

    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest_path.display()
    );

    // 2. Embed application icon + VS_VERSION_INFO resource via Windows SDK rc.exe.
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let ico_path = manifest_dir
        .join("assets")
        .join("branding")
        .join("vanguard-cv.ico");

    if ico_path.exists()
        && let Some(rc_exe) = find_windows_rc_exe()
    {
        let escaped_ico = ico_path.display().to_string().replace('\\', "\\\\");
        let rc_content = format!(
            r#"1 ICON "{escaped_ico}"

1 VERSIONINFO
FILEVERSION {ver_comma}
PRODUCTVERSION {ver_comma}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Omer Tawfig\0"
            VALUE "FileDescription", "VanguardCV — Privacy-First AI CV Studio\0"
            VALUE "FileVersion", "{ver_dot}\0"
            VALUE "InternalName", "vanguard-cv\0"
            VALUE "LegalCopyright", "Copyright (c) 2026 Omer Tawfig. All Rights Reserved.\0"
            VALUE "OriginalFilename", "vanguard-cv.exe\0"
            VALUE "ProductName", "VanguardCV\0"
            VALUE "ProductVersion", "{pkg_ver}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
        );

        let rc_path = out_dir.join("vanguard-cv.rc");
        let res_path = out_dir.join("vanguard-cv.res");
        if fs::write(&rc_path, rc_content).is_ok() {
            let status = Command::new(&rc_exe)
                .arg("/nologo")
                .arg("/fo")
                .arg(&res_path)
                .arg(&rc_path)
                .status();
            match status {
                Ok(s) if s.success() && res_path.exists() => {
                    println!("cargo:rustc-link-arg-bins={}", res_path.display());
                }
                Ok(s) => {
                    println!(
                        "cargo:warning=rc.exe exited with status {s}; continuing without embedded icon resource"
                    );
                }
                Err(err) => {
                    println!(
                        "cargo:warning=failed to run rc.exe ({err}); continuing without embedded icon resource"
                    );
                }
            }
        }
    }
}

fn find_windows_rc_exe() -> Option<PathBuf> {
    // 1. Check PATH first.
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Scan standard Windows Kits 10 SDK directories (newest version first).
    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = fs::read_dir(kits_bin) {
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|entry| entry.path()))
            .filter(|p| p.is_dir())
            .collect();
        versions.sort();
        versions.reverse();

        let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "arm64",
            Ok("x86") => "x86",
            _ => "x64",
        };
        for ver_dir in versions {
            let candidate = ver_dir.join(arch).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
            let fallback_x64 = ver_dir.join("x64").join("rc.exe");
            if fallback_x64.is_file() {
                return Some(fallback_x64);
            }
        }
    }

    None
}

/// Directories that may contain the versioned runtime libraries, most
/// specific first.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // Prefer the directories `ldconfig` resolves, when it is available.
    if let Ok(output) = Command::new("ldconfig").arg("-p").output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let Some((lib, path)) = line.split_once(" => ") else {
                continue;
            };
            if lib.trim().starts_with("libxkbcommon")
                && let Some(parent) = Path::new(path.trim()).parent()
            {
                dirs.push(parent.to_path_buf());
            }
        }
    }

    for dir in [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib/riscv64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
        "/lib/x86_64-linux-gnu",
        "/lib/aarch64-linux-gnu",
        "/lib64",
        "/lib",
        "/usr/local/lib",
    ] {
        let path = PathBuf::from(dir);
        if !dirs.contains(&path) {
            dirs.push(path);
        }
    }

    dirs
}
