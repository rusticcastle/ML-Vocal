// SPDX-License-Identifier: GPL-3.0-or-later
//! Build `ML Vocal` as VST3 (bundle tree) and CLAP (single file) into `target/bundled/`,
//! and optionally install to `~/.vst3` and `~/.clap` when `--install` is passed.
use std::{env, fs, path::Path, process::Command};

const PLUGINS: &[(&str, &str, &str)] = &[("lemon-vocal", "lemon_vocal", "ML Vocal")];

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let install = args.iter().any(|a| a == "--install");

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).args(["build", "--release"]);
    for (pkg, _, _) in PLUGINS {
        cmd.args(["-p", pkg]);
    }
    let status = cmd.status().expect("run cargo build");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    let rel = root.join("target/release");
    let out = root.join("target/bundled");
    fs::create_dir_all(&out).expect("mkdir target/bundled");

    for &(_, lib, display) in PLUGINS {
        let dylib = rel.join(dylib_name(lib));
        let vst3 = out.join(format!("{display}.vst3"));
        let _ = fs::remove_dir_all(&vst3);
        let bin_dir = vst3.join("Contents").join(vst3_arch());
        fs::create_dir_all(&bin_dir).expect("mkdir vst3 Contents");
        fs::copy(&dylib, bin_dir.join(format!("{display}{}", vst3_ext())))
            .expect("copy vst3 binary");
        #[cfg(target_os = "macos")]
        write_macos_plist(&vst3.join("Contents/Info.plist"), display, lib);

        let clap = out.join(format!("{display}.clap"));
        let _ = fs::remove_dir_all(&clap);
        let _ = fs::remove_file(&clap);
        #[cfg(target_os = "macos")]
        {
            let mac = clap.join("Contents/MacOS");
            fs::create_dir_all(&mac).expect("mkdir clap Contents/MacOS");
            fs::copy(&dylib, mac.join(display)).expect("copy clap binary");
            write_macos_plist(&clap.join("Contents/Info.plist"), display, lib);
        }
        #[cfg(not(target_os = "macos"))]
        fs::copy(&dylib, &clap).expect("copy clap binary");

        println!("{}", vst3.display());
        println!("{}", clap.display());
    }

    if install {
        let (vdir, cdir) = user_plugin_dirs();
        fs::create_dir_all(&vdir).expect("mkdir user vst3 dir");
        fs::create_dir_all(&cdir).expect("mkdir user clap dir");
        for &(_, _, display) in PLUGINS {
            let vn = format!("{display}.vst3");
            let cn = format!("{display}.clap");
            copy_any(&out.join(&vn), &vdir.join(&vn)).expect("install vst3");
            copy_any(&out.join(&cn), &cdir.join(&cn)).expect("install clap");
        }
        println!("installed to {} and {}", vdir.display(), cdir.display());
    }
}

fn copy_any(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_dir_all(dst);
    let _ = fs::remove_file(dst);
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for e in fs::read_dir(src)? {
            let e = e?;
            copy_any(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

fn user_plugin_dirs() -> (std::path::PathBuf, std::path::PathBuf) {
    #[cfg(target_os = "windows")]
    {
        let base = env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("C:\\Program Files\\Common Files"));
        let p = base.join("Programs").join("Common");
        (p.join("VST3"), p.join("CLAP"))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::path::PathBuf::from(env::var_os("HOME").unwrap());
        let p = home.join("Library/Audio/Plug-Ins");
        (p.join("VST3"), p.join("CLAP"))
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::path::PathBuf::from(env::var_os("HOME").unwrap());
        (home.join(".vst3"), home.join(".clap"))
    }
}

fn dylib_name(lib: &str) -> String {
    #[cfg(target_os = "windows")]
    return format!("{lib}.dll");
    #[cfg(target_os = "macos")]
    return format!("lib{lib}.dylib");
    #[cfg(target_os = "linux")]
    return format!("lib{lib}.so");
}

fn vst3_arch() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-win";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "arm64-win";
    #[cfg(target_os = "macos")]
    return "MacOS";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-linux";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-linux";
}

fn vst3_ext() -> &'static str {
    #[cfg(target_os = "windows")]
    return ".vst3";
    #[cfg(target_os = "macos")]
    return "";
    #[cfg(target_os = "linux")]
    return ".so";
}

#[cfg(target_os = "macos")]
fn write_macos_plist(path: &Path, display: &str, ident: &str) {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>{display}</string>
  <key>CFBundleIdentifier</key><string>studio.ml.plug.{ident}</string>
  <key>CFBundleName</key><string>{display}</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
</dict></plist>
"#
    );
    fs::write(path, xml).expect("write Info.plist");
}
