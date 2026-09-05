//! Explicit local process liveness observation. Never starts a backend and never
//! promotes a liveness response into vendor identity or model-access verification.
use std::path::Path;

#[derive(Debug, serde::Serialize)]
pub struct BackendLiveness {
    pub process_liveness_healthy: bool,
    pub process_binding_checked: bool,
    pub version: String,
    pub model_access_checked: bool,
    pub source_integrity_checked: bool,
}

pub fn observe_running_backend(
    root: &Path,
    pid: u32,
    port: u16,
    expected_version: &str,
) -> Result<BackendLiveness, String> {
    if pid == 0 || pid > i32::MAX as u32 || port == 0 || !valid_version(expected_version) {
        return Err("invalid PID, port or expected numeric version".into());
    }
    #[cfg(target_os = "macos")]
    {
        let before = macos_binding(root, pid, port)?;
        let version = request_health(port, expected_version)?;
        let after = macos_binding(root, pid, port)?;
        if before != after {
            return Err("backend process identity changed during health observation".into());
        }
        Ok(BackendLiveness {
            process_liveness_healthy: true,
            process_binding_checked: true,
            version,
            model_access_checked: false,
            source_integrity_checked: false,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = root;
        Err("bound backend observation currently requires macOS".into())
    }
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(any(target_os = "macos", test))]
fn request_health(port: u16, expected: &str) -> Result<String, String> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|_| "cannot create local health client")?;
    let response = client
        .get(format!("http://127.0.0.1:{port}/api/health"))
        .send()
        .map_err(|_| "local backend health request failed")?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("local health endpoint did not return HTTP 200".into());
    }
    let mut body = Vec::new();
    response
        .take(64 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| "cannot read local health response")?;
    if body.len() > 64 * 1024 {
        return Err("local health response exceeds size limit".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| "invalid health JSON")?;
    if value.get("ok").and_then(|v| v.as_bool()) != Some(true)
        || value.get("version").and_then(|v| v.as_str()) != Some(expected)
    {
        return Err("backend is not ready or its version differs from the expected version".into());
    }
    Ok(expected.to_owned())
}

#[cfg(target_os = "macos")]
fn macos_binding(
    root: &Path,
    pid: u32,
    port: u16,
) -> Result<(u64, u64, std::path::PathBuf), String> {
    use super::hermes::checked_path;
    use std::fs;
    let checkout = checked_path(root, "hermes-agent", false)?.ok_or("missing Hermes checkout")?;
    let python = checked_path(root, "hermes-agent/venv/bin/python", true)?
        .ok_or("missing Hermes runtime Python")?;
    let python = fs::canonicalize(python).map_err(|_| "cannot resolve runtime Python")?;
    // Read OS-owned executable and start-time data, never trust argv or a PID file.
    let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    // SAFETY: buffers have the exact capacities provided to the Darwin APIs;
    // proc_bsdinfo is read only after a complete successful write.
    let (path_length, info_length, current_uid) = unsafe {
        (
            libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32),
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<libc::proc_bsdinfo>() as i32,
            ),
            libc::geteuid(),
        )
    };
    if path_length <= 0 || info_length as usize != std::mem::size_of::<libc::proc_bsdinfo>() {
        return Err("cannot inspect backend process identity".into());
    }
    // SAFETY: proc_pidinfo reported a complete proc_bsdinfo above.
    let info = unsafe { info.assume_init() };
    if info.pbi_uid != current_uid {
        return Err("backend is not owned by the current user".into());
    }
    let end = buffer
        .iter()
        .position(|b| *b == 0)
        .ok_or("invalid process executable path")?;
    let executable =
        std::str::from_utf8(&buffer[..end]).map_err(|_| "invalid process executable path")?;
    let executable =
        fs::canonicalize(executable).map_err(|_| "cannot resolve process executable")?;
    if executable != python {
        return Err("PID does not execute this installation's Python interpreter".into());
    }
    let pid_arg = pid.to_string();
    let cwd = super::macos::command_output_with_timeout(
        "/usr/sbin/lsof",
        &["-a", "-p", &pid_arg, "-d", "cwd", "-Fn"],
        std::time::Duration::from_secs(3),
    )?;
    let cwd_text = String::from_utf8_lossy(&cwd.stdout);
    let expected_cwd = format!("n{}", checkout.to_str().ok_or("invalid checkout path")?);
    if !cwd.status.success() || !cwd_text.lines().any(|line| line == expected_cwd) {
        return Err("backend working directory does not match this checkout".into());
    }
    let port_arg = format!("-iTCP:{port}");
    let socket = super::macos::command_output_with_timeout(
        "/usr/sbin/lsof",
        &[
            "-nP",
            "-a",
            "-p",
            &pid_arg,
            &port_arg,
            "-sTCP:LISTEN",
            "-Fn",
        ],
        std::time::Duration::from_secs(3),
    )?;
    let expected_socket = format!("n127.0.0.1:{port}");
    if !socket.status.success()
        || !String::from_utf8_lossy(&socket.stdout)
            .lines()
            .any(|line| line == expected_socket)
    {
        return Err("PID does not own the requested IPv4 loopback listener".into());
    }
    Ok((info.pbi_start_tvsec, info.pbi_start_tvusec, executable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn binds_health_to_owned_python_checkout_and_listener() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let checkout = dir.path().join("hermes-agent");
        std::fs::create_dir_all(checkout.join("venv/bin")).unwrap();
        let python = Command::new("/usr/bin/python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(python.status.success());
        let python = String::from_utf8(python.stdout).unwrap().trim().to_owned();
        std::os::unix::fs::symlink(&python, checkout.join("venv/bin/python")).unwrap();
        // Controlled local fixture only: no Hermes or user configuration is loaded.
        let script = "import http.server\nclass Handler(http.server.BaseHTTPRequestHandler):\n def do_GET(self):\n  self.send_response(200); self.end_headers(); self.wfile.write(b'{\"ok\":true,\"version\":\"0.21.0\"}')\nserver=http.server.HTTPServer(('127.0.0.1',0),Handler)\nprint(server.server_port,flush=True)\nserver.serve_forever()\n";
        let mut child = ChildGuard(
            Command::new(&python)
                .args(["-I", "-u", "-c", script])
                .current_dir(&checkout)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let mut line = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line.trim().parse::<u16>().unwrap();
        // Apple's launcher execs a framework binary. The synthetic venv must
        // name that actual executable; the production comparison stays strict.
        let mut actual = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the writable buffer length is supplied exactly.
        let length = unsafe {
            libc::proc_pidpath(
                child.0.id() as i32,
                actual.as_mut_ptr().cast(),
                actual.len() as u32,
            )
        };
        assert!(length > 0);
        let end = actual.iter().position(|byte| *byte == 0).unwrap();
        let actual = std::str::from_utf8(&actual[..end]).unwrap();
        std::fs::remove_file(checkout.join("venv/bin/python")).unwrap();
        std::os::unix::fs::symlink(actual, checkout.join("venv/bin/python")).unwrap();
        let report = observe_running_backend(dir.path(), child.0.id(), port, "0.21.0").unwrap();
        assert!(report.process_liveness_healthy);
        assert!(report.process_binding_checked);
        assert!(!report.source_integrity_checked);
        assert!(!report.model_access_checked);
        assert!(observe_running_backend(dir.path(), std::process::id(), port, "0.21.0").is_err());
        assert!(observe_running_backend(dir.path(), child.0.id(), port, "0.20.0").is_err());
    }

    #[test]
    fn health_requires_boolean_ok_and_exact_version() {
        let server = httpmock::MockServer::start();
        let mut mock = server.mock(|when, then| {
            when.path("/api/health");
            then.status(200)
                .json_body(serde_json::json!({"ok":true,"version":"0.21.0"}));
        });
        assert_eq!(request_health(server.port(), "0.21.0").unwrap(), "0.21.0");
        assert!(request_health(server.port(), "0.20.0").is_err());
        mock.delete();
        server.mock(|when, then| {
            when.path("/api/health");
            then.status(200)
                .json_body(serde_json::json!({"ok":"true","version":"0.21.0"}));
        });
        assert!(request_health(server.port(), "0.21.0").is_err());
    }

    #[test]
    fn health_rejects_redirects_and_oversized_responses() {
        let server = httpmock::MockServer::start();
        let mut mock = server.mock(|when, then| {
            when.path("/api/health");
            then.status(302).header("Location", "http://127.0.0.1:1/");
        });
        assert!(request_health(server.port(), "0.21.0").is_err());
        mock.delete();
        server.mock(|when, then| {
            when.path("/api/health");
            then.status(200).body("x".repeat(65537));
        });
        assert!(request_health(server.port(), "0.21.0").is_err());
    }

    #[test]
    fn invalid_process_and_version_fail_before_network() {
        assert!(observe_running_backend(Path::new("/absent"), 0, 9000, "0.21.0").is_err());
        assert!(observe_running_backend(Path::new("/absent"), 1, 0, "0.21.0").is_err());
        assert!(observe_running_backend(Path::new("/absent"), 1, 9000, "main").is_err());
    }
}
