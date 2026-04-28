use std::env;
use std::path::PathBuf;
use std::process::Command;

fn pkg_config(args: &[&str]) -> Vec<String> {
    let mut pkg_path =
        env::var("PKG_CONFIG_PATH").unwrap_or_default();
    // Brew installs icu4c keg-only; auto-add its pkgconfig dir if present.
    for prefix in ["/opt/homebrew/opt/icu4c@78", "/opt/homebrew/opt/icu4c"] {
        let p = format!("{prefix}/lib/pkgconfig");
        if PathBuf::from(&p).exists() {
            if !pkg_path.is_empty() {
                pkg_path.push(':');
            }
            pkg_path.push_str(&p);
            break;
        }
    }
    let out = Command::new("pkg-config")
        .env("PKG_CONFIG_PATH", &pkg_path)
        .args(args)
        .output()
        .expect("pkg-config failed (is it installed?)");
    if !out.status.success() {
        panic!(
            "pkg-config {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn main() {
    println!("cargo:rerun-if-changed=cpp");
    println!("cargo:rerun-if-changed=build.rs");

    let cflags_xapian = pkg_config(&["--cflags", "xapian-core"]);
    let libs_xapian = pkg_config(&["--libs", "xapian-core"]);
    let cflags_icu = pkg_config(&["--cflags", "icu-uc", "icu-i18n"]);
    let libs_icu = pkg_config(&["--libs", "icu-uc", "icu-i18n"]);

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-Wno-deprecated-declarations")
        .file("cpp/htmlparse.cc")
        .file("cpp/myhtmlparse.cc")
        .file("cpp/bridge.cc");

    for f in cflags_xapian.iter().chain(cflags_icu.iter()) {
        if let Some(rest) = f.strip_prefix("-I") {
            build.include(rest);
        } else {
            build.flag(f);
        }
    }
    build.compile("xapianbuilder_cpp");

    for f in libs_xapian.iter().chain(libs_icu.iter()) {
        if let Some(rest) = f.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={rest}");
        } else if let Some(rest) = f.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={rest}");
        } else if f.starts_with("-framework") {
            // pass through
        }
    }
    // Link C++ runtime explicitly on macOS/Linux.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
