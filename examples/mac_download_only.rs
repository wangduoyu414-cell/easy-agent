//! Download the official verified Mac DMG into the user's Downloads folder.
use easy_agent::core::{Architecture, ProductId};
use std::sync::atomic::AtomicBool;
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (product, arch) = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["hermes", "arm64"] => (ProductId::Hermes, Architecture::Arm64),
        ["clash", "arm64"] => (ProductId::ClashVergeRev, Architecture::Arm64),
        ["clash", "x64"] => (ProductId::ClashVergeRev, Architecture::X64),
        _ => return Err("usage: mac_download_only hermes arm64 | clash arm64 | clash x64".into()),
    };
    let file = easy_agent::mac_download::save(product, arch, &AtomicBool::new(false), |_| {})?;
    println!("{}", file.display());
    Ok(())
}
