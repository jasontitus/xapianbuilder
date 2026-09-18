use std::env;
use std::path::PathBuf;
use std::process::Command;

fn add_homebrew_icu_path() {
    let host = env::var("HOST").expect("Cargo sets HOST");
    let target = env::var("TARGET").expect("Cargo sets TARGET");
    if host != target || !target.contains("apple-darwin") {
        return;
    }

    // Explicit pkg-config configuration takes precedence, including an empty
    // value. Do not inject host paths into a cross-compilation environment.
    let target_underscores = target.replace('-', "_");
    let mut configured = false;
    for name in [
        "PKG_CONFIG_PATH",
        "PKG_CONFIG_LIBDIR",
        "PKG_CONFIG_SYSROOT_DIR",
    ] {
        for key in [
            name.to_owned(),
            format!("HOST_{name}"),
            format!("{name}_{target}"),
            format!("{name}_{target_underscores}"),
        ] {
            println!("cargo:rerun-if-env-changed={key}");
            configured |= env::var_os(&key).is_some();
        }
    }
    if configured {
        return;
    }

    println!("cargo:rerun-if-env-changed=PATH");
    let brew_prefix = Command::new("brew")
        .args(["--prefix", "icu4c"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|prefix| PathBuf::from(prefix.trim()));
    for prefix in brew_prefix.into_iter().chain([
        PathBuf::from("/opt/homebrew/opt/icu4c"),
        PathBuf::from("/usr/local/opt/icu4c"),
    ]) {
        let path = prefix.join("lib/pkgconfig");
        if path.is_dir() {
            env::set_var("PKG_CONFIG_PATH", path);
            return;
        }
    }
}

fn probe(package: &str, metadata: bool) -> pkg_config::Library {
    pkg_config::Config::new()
        .cargo_metadata(metadata)
        .probe(package)
        .unwrap_or_else(|error| {
            panic!("Cannot discover {package}: {error}\nInstall Xapian and ICU development packages and pkg-config; see README.md.")
        })
}

fn main() {
    println!("cargo:rerun-if-changed=cpp");
    println!("cargo:rerun-if-changed=build.rs");
    add_homebrew_icu_path();

    let packages = ["xapian-core", "icu-uc", "icu-i18n"];
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-Wno-deprecated-declarations")
        .file("cpp/htmlparse.cc")
        .file("cpp/myhtmlparse.cc")
        .file("cpp/bridge.cc");
    for package in packages {
        let library = probe(package, false);
        for include in library.include_paths {
            build.include(include);
        }
        for (name, value) in library.defines {
            build.define(&name, value.as_deref());
        }
    }
    build.compile("xapianbuilder_cpp");

    // Emit dependency links after the wrapper archive, using the crate's full
    // handling of frameworks, static dependencies, and platform linker flags.
    for package in packages {
        probe(package, true);
    }
    // cc selects the target's C++ runtime and honors CXXSTDLIB overrides.
}
