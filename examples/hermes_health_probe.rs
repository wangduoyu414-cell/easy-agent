fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        eprintln!("usage: hermes_health_probe HERMES_HOME BACKEND_PID PORT EXPECTED_VERSION");
        return std::process::ExitCode::FAILURE;
    }
    let result = args[1]
        .parse::<u32>()
        .map_err(|_| "invalid PID".to_owned())
        .and_then(|pid| {
            args[2]
                .parse::<u16>()
                .map_err(|_| "invalid port".to_owned())
                .map(|port| (pid, port))
        })
        .and_then(|(pid, port)| {
            easy_agent::platform::hermes_health::observe_running_backend(
                std::path::Path::new(&args[0]),
                pid,
                port,
                &args[3],
            )
        });
    match result {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Hermes health observation failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
