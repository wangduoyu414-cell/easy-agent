fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        eprintln!("usage: hermes_desktop_probe /absolute/path/to/Hermes.app");
        return std::process::ExitCode::FAILURE;
    }
    match easy_agent::platform::hermes::observe_desktop_identity(
        std::path::Path::new(&args[0]),
        easy_agent::core::Architecture::Arm64,
    ) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Hermes desktop observation failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
