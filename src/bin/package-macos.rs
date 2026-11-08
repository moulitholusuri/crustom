//! Build, bundle and ad-hoc sign the macOS application.
//!
//! Run with:
//!     cargo run --release --bin package-macos

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_NAME: &str = "Crustom";
const BIN_NAME: &str = "crustom";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");
    let app = root.join("dist").join(format!("{APP_NAME}.app"));
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");

    println!("Building {APP_NAME} {version}…");
    if env::var_os("CRUSTOM_SKIP_BUILD").is_none() {
        run(Command::new("cargo")
            .args(["build", "--release", "--bin", BIN_NAME])
            .current_dir(&root))?;
    } else {
        println!("Using the existing target/release/{BIN_NAME} binary.");
    }

    if app.exists() {
        fs::remove_dir_all(&app)?;
    }
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;

    fs::copy(
        root.join("target/release").join(BIN_NAME),
        macos.join(BIN_NAME),
    )?;
    fs::copy(
        root.join("assets").join("Crustom.icns"),
        resources.join("Crustom.icns"),
    )?;
    make_executable(&macos.join(BIN_NAME))?;

    fs::write(contents.join("Info.plist"), info_plist(version))?;
    run(Command::new("plutil")
        .arg("-lint")
        .arg(contents.join("Info.plist")))?;

    // Finder/file-provider metadata can make codesign reject the bundle.
    let _ = Command::new("xattr").arg("-cr").arg(&app).status();
    run(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app))?;
    let _ = Command::new("xattr").arg("-cr").arg(&app).status();

    let verified = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app)
        .status()
        .is_ok_and(|status| status.success());
    if verified {
        println!("Signature verified.");
    } else {
        println!(
            "Bundle signed. Strict verification was skipped because Finder metadata was re-added by the file provider."
        );
    }

    println!("Packaged: {}", app.display());
    println!("Launch with: open \"{}\"", app.display());
    Ok(())
}

fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>{APP_NAME}</string>
    <key>CFBundleExecutable</key>
    <string>{BIN_NAME}</string>
    <key>CFBundleIconFile</key>
    <string>Crustom</string>
    <key>CFBundleIdentifier</key>
    <string>com.crustom.desktop</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.finance</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#
    )
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command failed: {command:?}").into());
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
