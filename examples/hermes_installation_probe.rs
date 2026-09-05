use easy_agent::core::{Architecture, OperatingSystem};
use easy_agent::platform::hermes::observe_installation;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        eprintln!(
            "usage: hermes_installation_probe HERMES_HOME macos|windows arm64|x64 [EXPECTED_FULL_COMMIT]"
        );
        return std::process::ExitCode::FAILURE;
    }
    let os = match args[1].as_str() {
        "macos" => OperatingSystem::MacOs,
        "windows" => OperatingSystem::Windows,
        _ => OperatingSystem::Unsupported,
    };
    let architecture = match args[2].as_str() {
        "arm64" => Architecture::Arm64,
        "x64" => Architecture::X64,
        _ => Architecture::Unsupported,
    };
    match observe_installation(
        std::path::Path::new(&args[0]),
        os,
        architecture,
        args.get(3).map(String::as_str),
    ) {
        Ok(observation) => {
            println!("{}", serde_json::to_string_pretty(&observation).unwrap());
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Hermes observation failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
