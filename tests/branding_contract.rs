use std::fs;
use std::path::Path;

use easy_agent::{APP_ID, APP_NAME};
use plist::Value;

const BRAND_ICON: &[u8] = include_bytes!("../assets/branding/easy-agent-icon-512.png");

#[test]
fn easy_agent_branding_is_consistent_across_runtime_and_packages() {
    assert_eq!(APP_NAME, "easy agent");
    assert_eq!(APP_ID, "io.github.wangduoyu414-cell.easy-agent");

    let runtime_icon = eframe::icon_data::from_png_bytes(BRAND_ICON).unwrap();
    assert_eq!((runtime_icon.width, runtime_icon.height), (512, 512));
    assert_eq!(runtime_icon.rgba.len(), 512 * 512 * 4);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative_path in [
        "assets/branding/easy-agent-icon.png",
        "assets/branding/easy-agent-icon-512.png",
        "assets/branding/easy-agent.ico",
        "packaging/macos/easy-agent.icns",
    ] {
        assert!(
            root.join(relative_path).is_file(),
            "missing {relative_path}"
        );
    }

    let plist = Value::from_file(root.join("packaging/macos/Info.plist")).unwrap();
    let dictionary = plist.as_dictionary().unwrap();
    assert_eq!(
        dictionary
            .get("CFBundleDisplayName")
            .and_then(Value::as_string),
        Some(APP_NAME)
    );
    assert_eq!(
        dictionary
            .get("CFBundleIdentifier")
            .and_then(Value::as_string),
        Some(APP_ID)
    );
    assert_eq!(
        dictionary
            .get("CFBundleExecutable")
            .and_then(Value::as_string),
        Some("easy-agent")
    );
    assert_eq!(
        dictionary
            .get("CFBundleIconFile")
            .and_then(Value::as_string),
        Some("easy-agent.icns")
    );

    let macos_package = fs::read_to_string(root.join("packaging/build-macos.sh")).unwrap();
    assert!(macos_package.contains("easy-agent-macos-universal.dmg"));
    assert!(macos_package.contains("Resources/$icon_name"));
    let windows_package = fs::read_to_string(root.join("packaging/build-windows.ps1")).unwrap();
    assert!(windows_package.contains("$binaryName = 'easy-agent'"));
    let windows_resource = fs::read_to_string(root.join("build.rs")).unwrap();
    assert!(windows_resource.contains("assets/branding/easy-agent.ico"));
}
