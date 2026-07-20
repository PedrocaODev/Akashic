use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_akashic")
}

fn root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("akashic-checkpoint-{stamp}"));
    fs::create_dir(&root).expect("root");
    fs::create_dir_all(root.join("config")).expect("config");
    fs::create_dir_all(root.join("home")).expect("home");
    root
}

fn socket_path(runtime: &Path) -> PathBuf {
    runtime.join("akashic/daemon.sock")
}

fn command(root: &Path, args: &[&str]) -> Command {
    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    let mut command = Command::new(binary());
    command
        .args(args)
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .env_remove("AKASHIC_LOG_LEVEL")
        .env_remove("AKASHIC_SHUTDOWN_TIMEOUT_SECONDS")
        .env_remove("AKASHIC_JSONL_MAX_LINE_BYTES");
    command
}

fn start_daemon(root: &Path) -> Child {
    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    let mut child = command(root, &["daemon"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    for _ in 0..100 {
        if UnixStream::connect(socket_path(&runtime)).is_ok() {
            return child;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let output = child.wait_with_output().expect("daemon output");
    panic!(
        "daemon socket did not appear at {}: {:?}",
        root.display(),
        output.stderr
    );
}

fn stop_daemon(mut daemon: Child) {
    daemon.kill().expect("stop daemon");
    let _ = daemon.wait_with_output().expect("wait daemon");
}

fn output(command: &mut Command) -> Output {
    command.output().expect("run akashic")
}

fn fake_socket(root: &Path, responses: &[&'static str]) -> thread::JoinHandle<()> {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("socket directory");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let listener = UnixListener::bind(socket_path(&runtime)).expect("fake socket");
    let responses = responses.to_vec();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("client");
        for response in responses {
            let mut request = Vec::new();
            loop {
                let mut byte = [0; 1];
                if stream.read_exact(&mut byte).is_err() {
                    return;
                }
                request.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).expect("response");
        }
    })
}

#[test]
fn explicit_cli_overrides_are_validated_for_version() {
    let root = root();
    let result = output(&mut command(&root, &["--log-level", "invalid", "version"]));
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(
        result.stderr,
        b"{\"code\":\"config.invalid\",\"message\":\"invalid configuration\",\"retryable\":false}\n"
    );
}

#[test]
fn doctor_preserves_handshake_error_code() {
    let root = root();
    let server = handshake_error_server(&root, "__request__");
    let result = output(&mut command(&root, &["doctor"]));
    server.join().expect("fake daemon");
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"code\":\"authorization.peer_uid\""));
    assert!(!body.contains("\"code\":\"protocol.malformed\""));
}

#[test]
fn doctor_preserves_health_error_code() {
    let root = root();
    let server = health_error_server(&root);
    let result = output(&mut command(&root, &["doctor"]));
    server.join().expect("fake daemon");
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"code\":\"protocol.unsupported_version\""));
}

#[test]
fn incomplete_jsonl_frame_returns_structured_error() {
    let root = root();
    let daemon = start_daemon(&root);
    let input = b"{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{\"client_role\":\"jsonl\",\"client_version\":\"0.1.0\",\"client_instance_id\":\"22222222-2222-4222-8222-222222222222\"}}";
    let mut client = command(&root, &["run", "--jsonl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("jsonl");
    client
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("input");
    let result = client.wait_with_output().expect("wait jsonl");
    stop_daemon(daemon);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert!(result.status.success());
    assert!(body.contains("\"kind\":\"error.response\""));
    assert!(body.contains("\"code\":\"protocol.malformed\""));
    assert!(!body.contains("\"kind\":\"handshake.response\""));
}

#[test]
fn invalid_utf8_jsonl_frame_returns_structured_error() {
    let root = root();
    let daemon = start_daemon(&root);
    let mut client = command(&root, &["run", "--jsonl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("jsonl");
    client
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"\xff\n")
        .expect("input");
    let result = client.wait_with_output().expect("wait jsonl");
    stop_daemon(daemon);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert!(result.status.success());
    assert!(body.contains("\"kind\":\"error.response\""));
    assert!(body.contains("\"code\":\"protocol.malformed\""));
}

#[test]
fn tui_protocol_startup_failure_is_structured_stderr() {
    let root = root();
    let server = handshake_error_server(&root, "__request__");
    let result = output(&mut command(&root, &["tui"]));
    server.join().expect("fake daemon");
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert_eq!(
        result.stderr,
        b"{\"code\":\"authorization.peer_uid\",\"message\":\"protocol handshake failed\",\"retryable\":false}\n"
    );
}

#[test]
fn tui_quit_exits_without_eof() {
    let root = root();
    let daemon = start_daemon(&root);
    let mut client = command(&root, &["tui"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("tui");
    let mut stdin = client.stdin.take().expect("stdin");
    stdin.write_all(b"q\n").expect("quit");
    let mut finished = false;
    for _ in 0..100 {
        if client.try_wait().expect("poll tui").is_some() {
            finished = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !finished {
        let _ = client.kill();
    }
    let result = client.wait_with_output().expect("wait tui");
    stop_daemon(daemon);
    assert!(finished, "TUI did not quit on q input");
    assert!(result.status.success());
}

#[test]
fn invalid_runtime_is_not_reported_as_safe_degraded_doctor() {
    let root = root();
    let runtime = root.join("runtime-file");
    fs::write(&runtime, b"not a directory").expect("runtime file");
    let state = root.join("state-file");
    fs::write(&state, b"not a state directory").expect("state file");
    let mut command = Command::new(binary());
    command
        .arg("doctor")
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"));
    let result = output(&mut command);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"status\":\"error\""));
    assert!(body.contains("\"code\":\"config.invalid\""));
    assert!(!body.contains("\"status\":\"degraded\""));
}

#[test]
fn invalid_request_uuid_is_rejected_without_correlation_injection() {
    let root = root();
    let daemon = start_daemon(&root);
    let input = b"{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"not-a-uuid\",\"payload\":{\"client_role\":\"jsonl\",\"client_version\":\"0.1.0\",\"client_instance_id\":\"22222222-2222-4222-8222-222222222222\"}}\n";
    let mut client = command(&root, &["run", "--jsonl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("jsonl");
    client
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("input");
    let result = client.wait_with_output().expect("wait jsonl");
    stop_daemon(daemon);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert!(body.contains("\"code\":\"protocol.malformed\""));
    assert!(!body.contains("\"correlation_id\":\"not-a-uuid\""));
    assert!(body.contains("\"correlation_id\":\"00000000-0000-4000-8000-"));
}

#[test]
fn only_top_level_request_id_can_be_used_for_correlation() {
    let root = root();
    let daemon = start_daemon(&root);
    let input = b"{\"payload\":{\"id\":\"11111111-1111-4111-8111-111111111111\",\"client_role\":\"jsonl\",\"client_version\":\"0.1.0\",\"client_instance_id\":\"22222222-2222-4222-8222-222222222222\"},\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\"}\n";
    let mut client = command(&root, &["run", "--jsonl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("jsonl");
    client
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("input");
    let result = client.wait_with_output().expect("wait jsonl");
    stop_daemon(daemon);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert!(body.contains("\"code\":\"protocol.malformed\""));
    assert!(!body.contains("\"correlation_id\":\"11111111-1111-4111-8111-111111111111\""));
}

#[test]
fn oversized_jsonl_frame_preserves_a_safe_top_level_request_id() {
    let root = root();
    let daemon = start_daemon(&root);
    let padding = "x".repeat(1_100_000);
    let input = format!(
        "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{{\"padding\":\"{padding}\"}}}}\n"
    );
    let mut client = command(&root, &["run", "--jsonl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("jsonl");
    client
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("input");
    let result = client.wait_with_output().expect("wait jsonl");
    stop_daemon(daemon);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert!(body.contains("\"code\":\"protocol.oversized\""));
    assert!(body.contains("\"correlation_id\":\"11111111-1111-4111-8111-111111111111\""));
}

#[test]
fn doctor_rejects_a_response_with_both_payload_and_error() {
    let root = root();
    let malformed = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.response\",\"id\":\"33333333-3333-4333-8333-333333333333\",\"correlation_id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{},\"error\":{\"code\":\"authorization.peer_uid\",\"message\":\"bad\",\"retryable\":false}}\n";
    let valid_health = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"health.response\",\"id\":\"66666666-6666-4666-8666-666666666666\",\"correlation_id\":\"55555555-5555-4555-8555-555555555555\",\"payload\":{\"status\":\"ok\",\"daemon_version\":\"0.1.0\",\"daemon_instance_id\":\"44444444-4444-4444-8444-444444444444\",\"protocol_version\":1}}\n";
    let server = fake_socket(&root, &[malformed, valid_health]);
    let result = output(&mut command(&root, &["doctor"]));
    server.join().expect("fake daemon");
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"status\":\"error\""));
    assert!(body.contains("\"code\":\"protocol.malformed\""));
}

#[test]
fn tui_rejects_a_response_with_both_payload_and_error() {
    let root = root();
    let malformed = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.response\",\"id\":\"33333333-3333-4333-8333-333333333333\",\"correlation_id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{},\"error\":{\"code\":\"authorization.peer_uid\",\"message\":\"bad\",\"retryable\":false}}\n";
    let valid_health = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"health.response\",\"id\":\"66666666-6666-4666-8666-666666666666\",\"correlation_id\":\"55555555-5555-4555-8555-555555555555\",\"payload\":{\"status\":\"ok\",\"daemon_version\":\"0.1.0\",\"daemon_instance_id\":\"44444444-4444-4444-8444-444444444444\",\"protocol_version\":1}}\n";
    let server = fake_socket(&root, &[malformed, valid_health]);
    let result = output(&mut command(&root, &["tui"]));
    server.join().expect("fake daemon");
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(result
        .stderr
        .windows(b"protocol.malformed".len())
        .any(|window| window == b"protocol.malformed"));
}

#[test]
fn symlinked_runtime_path_is_a_doctor_error() {
    let root = root();
    let target = root.join("runtime-target");
    fs::create_dir(&target).expect("runtime target");
    let runtime = root.join("runtime-link");
    std::os::unix::fs::symlink(&target, &runtime).expect("runtime symlink");
    let state = root.join("state-file");
    fs::write(&state, b"not a state directory").expect("state file");
    let mut command = Command::new(binary());
    command
        .arg("doctor")
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"));
    let result = output(&mut command);
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"status\":\"error\""));
    assert!(body.contains("\"code\":\"config.invalid\""));
}

#[test]
fn inaccessible_runtime_path_is_a_doctor_error_when_not_root() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let root = root();
    if fs::metadata(&root).expect("root metadata").uid() == 0 {
        return;
    }
    let runtime = root.join("runtime");
    fs::create_dir(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o000)).expect("permissions");
    let state = root.join("state-file");
    fs::write(&state, b"not a state directory").expect("state file");
    let mut command = Command::new(binary());
    command
        .arg("doctor")
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"));
    let result = output(&mut command);
    let _ = fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700));
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"status\":\"error\""));
    assert!(body.contains("\"code\":\"config.invalid\""));
}

#[derive(Clone, Copy)]
enum CorrelationMismatch {
    Handshake,
    Health,
}

fn read_request_id(stream: &mut UnixStream) -> String {
    let mut request = Vec::new();
    loop {
        let mut byte = [0; 1];
        stream.read_exact(&mut byte).expect("request");
        request.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    let request = String::from_utf8(request).expect("request utf8");
    let marker = "\"id\":\"";
    let start = request.find(marker).expect("request id") + marker.len();
    request[start..]
        .split('"')
        .next()
        .expect("request id value")
        .to_string()
}

fn correlation_server(root: &Path, mismatch: CorrelationMismatch) -> thread::JoinHandle<()> {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("socket directory");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let listener = UnixListener::bind(socket_path(&runtime)).expect("fake socket");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("client");
        let handshake_id = read_request_id(&mut stream);
        let handshake_correlation = match mismatch {
            CorrelationMismatch::Handshake => "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            CorrelationMismatch::Health => &handshake_id,
        };
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.response\",\"id\":\"33333333-3333-4333-8333-333333333333\",\"correlation_id\":\"{handshake_correlation}\",\"payload\":{{\"daemon_version\":\"0.1.0\",\"daemon_instance_id\":\"44444444-4444-4444-8444-444444444444\",\"protocol_version\":1,\"capabilities\":[\"health\"]}}}}"
        )
        .expect("handshake response");
        if matches!(mismatch, CorrelationMismatch::Handshake) {
            return;
        }
        let health_id = read_request_id(&mut stream);
        let health_correlation = match mismatch {
            CorrelationMismatch::Handshake => &health_id,
            CorrelationMismatch::Health => "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        };
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"health.response\",\"id\":\"66666666-6666-4666-8666-666666666666\",\"correlation_id\":\"{health_correlation}\",\"payload\":{{\"status\":\"ok\",\"daemon_version\":\"0.1.0\",\"daemon_instance_id\":\"44444444-4444-4444-8444-444444444444\",\"protocol_version\":1}}}}"
        )
        .expect("health response");
    })
}

fn health_error_server(root: &Path) -> thread::JoinHandle<()> {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("socket directory");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let listener = UnixListener::bind(socket_path(&runtime)).expect("fake socket");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("client");
        let handshake_id = read_request_id(&mut stream);
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.response\",\"id\":\"33333333-3333-4333-8333-333333333333\",\"correlation_id\":\"{handshake_id}\",\"payload\":{{\"daemon_version\":\"0.1.0\",\"daemon_instance_id\":\"44444444-4444-4444-8444-444444444444\",\"protocol_version\":1,\"capabilities\":[\"health\"]}}}}"
        )
        .expect("handshake response");
        let health_id = read_request_id(&mut stream);
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"error.response\",\"id\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\",\"correlation_id\":\"{health_id}\",\"error\":{{\"code\":\"protocol.unsupported_version\",\"message\":\"health rejected\",\"retryable\":false}}}}"
        )
        .expect("health response");
    })
}

fn handshake_error_server(root: &Path, correlation: &str) -> thread::JoinHandle<()> {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("socket directory");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let listener = UnixListener::bind(socket_path(&runtime)).expect("fake socket");
    let correlation = correlation.to_string();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("client");
        let request_id = read_request_id(&mut stream);
        let correlation = if correlation == "__request__" {
            request_id
        } else {
            correlation
        };
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"error.response\",\"id\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\",\"correlation_id\":\"{correlation}\",\"error\":{{\"code\":\"authorization.peer_uid\",\"message\":\"peer rejected\",\"retryable\":false}}}}"
        )
        .expect("handshake error");
    })
}

#[test]
fn doctor_rejects_mismatched_handshake_correlation_id() {
    let root = root();
    let server = correlation_server(&root, CorrelationMismatch::Handshake);
    let result = output(&mut command(&root, &["doctor"]));
    server.join().expect("fake daemon");
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"code\":\"protocol.malformed\""));
}

#[test]
fn doctor_rejects_mismatched_health_correlation_id() {
    let root = root();
    let server = correlation_server(&root, CorrelationMismatch::Health);
    let result = output(&mut command(&root, &["doctor"]));
    server.join().expect("fake daemon");
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"code\":\"protocol.malformed\""));
}

#[test]
fn tui_rejects_mismatched_handshake_correlation_id() {
    let root = root();
    let server = correlation_server(&root, CorrelationMismatch::Handshake);
    let result = output(&mut command(&root, &["tui"]));
    server.join().expect("fake daemon");
    assert_eq!(result.status.code(), Some(3));
    assert!(result
        .stderr
        .windows(b"protocol.malformed".len())
        .any(|window| window == b"protocol.malformed"));
}

#[test]
fn tui_rejects_mismatched_health_correlation_id() {
    let root = root();
    let server = correlation_server(&root, CorrelationMismatch::Health);
    let result = output(&mut command(&root, &["tui"]));
    server.join().expect("fake daemon");
    assert_eq!(result.status.code(), Some(3));
    assert!(result
        .stderr
        .windows(b"protocol.malformed".len())
        .any(|window| window == b"protocol.malformed"));
}

#[test]
fn doctor_rejects_mismatched_error_correlation_id_before_propagating_code() {
    let root = root();
    let server = handshake_error_server(&root, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    let result = output(&mut command(&root, &["doctor"]));
    server.join().expect("fake daemon");
    let body = String::from_utf8(result.stdout).expect("utf8");
    assert_eq!(result.status.code(), Some(1));
    assert!(body.contains("\"code\":\"protocol.malformed\""));
    assert!(!body.contains("\"code\":\"authorization.peer_uid\""));
}

#[test]
fn tui_rejects_mismatched_error_correlation_id_before_propagating_code() {
    let root = root();
    let server = handshake_error_server(&root, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    let result = output(&mut command(&root, &["tui"]));
    server.join().expect("fake daemon");
    assert_eq!(result.status.code(), Some(3));
    assert!(result
        .stderr
        .windows(b"protocol.malformed".len())
        .any(|window| window == b"protocol.malformed"));
    assert!(!result
        .stderr
        .windows(b"authorization.peer_uid".len())
        .any(|window| window == b"authorization.peer_uid"));
}

fn tui_error_server(root: &Path, code: &'static str) -> thread::JoinHandle<()> {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("socket directory");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let listener = UnixListener::bind(socket_path(&runtime)).expect("fake socket");
    let retryable = code.starts_with("lifecycle.") || code == "internal.unexpected";
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("client");
        let request_id = read_request_id(&mut stream);
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"error.response\",\"id\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\",\"correlation_id\":\"{request_id}\",\"error\":{{\"code\":\"{code}\",\"message\":\"startup failure\",\"retryable\":{retryable}}}}}"
        )
        .expect("startup error");
    })
}

#[test]
fn tui_error_response_codes_use_locked_startup_exit_mapping() {
    let cases = [
        ("config.invalid", 2),
        ("config.secret_forbidden", 2),
        ("protocol.malformed", 3),
        ("authorization.peer_uid", 3),
        ("lifecycle.daemon_running", 4),
        ("lifecycle.shutdown_timeout", 124),
        ("internal.unexpected", 1),
    ];
    for (code, expected_exit) in cases {
        let root = root();
        let server = tui_error_server(&root, code);
        let result = output(&mut command(&root, &["tui"]));
        server.join().expect("fake daemon");
        assert_eq!(result.status.code(), Some(expected_exit), "{code}");
        assert!(result.stdout.is_empty(), "{code}");
        assert!(
            result
                .stderr
                .windows(code.len())
                .any(|window| window == code.as_bytes()),
            "{code}"
        );
    }
}
