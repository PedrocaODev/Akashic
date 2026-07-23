use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERSION: &str = "0.1.0";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_akashic")
}

fn runtime_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("akashic-test-{}-{stamp}", std::process::id()));
    fs::create_dir(&path).expect("runtime directory");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    path
}

fn run(args: &[&str], runtime: &Path) -> Output {
    Command::new(binary())
        .args(args)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_STATE_HOME", runtime.join("state"))
        .output()
        .expect("run akashic")
}

fn socket_path(runtime: &Path) -> PathBuf {
    runtime.join("akashic").join("daemon.sock")
}

fn wait_for_socket(path: &Path) {
    for _ in 0..100 {
        if UnixStream::connect(path).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("daemon socket did not appear: {}", path.display());
}

fn start_daemon(runtime: &Path) -> Child {
    let child = Command::new(binary())
        .arg("daemon")
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_STATE_HOME", runtime.join("state"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start daemon");
    wait_for_socket(&socket_path(runtime));
    child
}

fn stop_daemon(mut daemon: Child) -> Output {
    daemon.kill().expect("stop daemon");
    daemon.wait_with_output().expect("wait for daemon")
}

#[test]
fn package_metadata_is_apache_licensed() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("Cargo.toml");
    assert!(manifest.contains("name = \"akashic\""));
    assert!(manifest.contains("version = \"0.1.0\""));
    assert!(manifest.contains("license = \"Apache-2.0\""));
}

#[test]
fn version_is_exact_and_does_not_need_a_daemon() {
    let runtime = runtime_dir();
    let output = run(&["version"], &runtime);
    assert!(output.status.success());
    assert_eq!(output.stdout, format!("akashic {VERSION}\n").as_bytes());
    assert!(output.stderr.is_empty());
    assert!(!socket_path(&runtime).exists());
}

#[test]
fn invalid_usage_is_one_stderr_error() {
    let runtime = runtime_dir();
    for args in [
        ["unknown"].as_slice(),
        ["run"].as_slice(),
        ["version", "doctor"].as_slice(),
    ] {
        let output = run(args, &runtime);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            b"{\"code\":\"usage.invalid\",\"message\":\"invalid usage\",\"retryable\":false}\n"
        );
    }
}

#[test]
fn doctor_without_daemon_is_one_degraded_result() {
    let runtime = runtime_dir();
    let output = run(&["doctor"], &runtime);
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(stdout.lines().count(), 1);
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("\"status\":\"degraded\""));
    assert!(stdout.contains("\"protocol\":{\"identifier\":\"akashic.local\",\"version\":1}"));
    assert!(stdout.contains("\"name\":\"daemon\""));
    assert!(stdout.contains("\"status\":\"warning\""));
    assert!(stdout.contains("\"code\":\"lifecycle.daemon_unavailable\""));
    assert!(stdout.contains("\"message\":\"daemon unavailable\""));
}

#[test]
fn doctor_socket_failure_is_one_error_result() {
    let runtime = runtime_dir();
    let dir = runtime.join("akashic");
    fs::create_dir(&dir).expect("akashic directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("akashic mode");
    fs::write(dir.join("daemon.sock"), b"not a socket").expect("fake socket");

    let output = run(&["doctor"], &runtime);
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout.lines().count(), 1);
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("\"status\":\"error\""));
    assert!(stdout.contains("\"status\":\"error\""));
    assert!(stdout.contains("\"code\":\"protocol.malformed\""));
    assert!(!stdout.contains("usage.invalid"));
}

#[test]
fn invalid_doctor_syntax_stays_on_generic_stderr() {
    let runtime = runtime_dir();
    let output = run(&["doctor", "--bad"], &runtime);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"{\"code\":\"usage.invalid\",\"message\":\"invalid usage\",\"retryable\":false}\n"
    );
}

#[test]
fn unavailable_jsonl_startup_error_stays_on_stderr() {
    let runtime = runtime_dir();
    let output = run(&["run", "--jsonl"], &runtime);
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"{\"code\":\"lifecycle.daemon_unavailable\",\"message\":\"daemon unavailable\",\"retryable\":true}\n"
    );
}

#[test]
fn doctor_handshake_failure_is_one_error_result() {
    let runtime = runtime_dir();
    let dir = runtime.join("akashic");
    fs::create_dir(&dir).expect("akashic directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("akashic mode");
    let listener = UnixListener::bind(dir.join("daemon.sock")).expect("socket");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("doctor connection");
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"not a handshake\n")
            .expect("bad handshake");
    });

    let output = run(&["doctor"], &runtime);
    server.join().expect("fake daemon");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout.lines().count(), 1);
    assert!(output.stderr.is_empty());
    assert!(stdout.contains("\"status\":\"error\""));
    assert!(stdout.contains("\"name\":\"handshake\""));
    assert!(stdout.contains("\"code\":\"protocol.malformed\""));
}

#[test]
fn daemon_jsonl_and_tui_expose_only_bootstrap_contracts() {
    let runtime = runtime_dir();
    let daemon = start_daemon(&runtime);

    let mut jsonl = Command::new(binary())
        .args(["run", "--jsonl"])
        .env("XDG_RUNTIME_DIR", &runtime)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start jsonl");
    let input = concat!(
        "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{\"client_role\":\"jsonl\",\"client_version\":\"0.1.0\",\"client_instance_id\":\"22222222-2222-4222-8222-222222222222\"}}\n",
        "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"unsupported.request\",\"id\":\"99999999-9999-4999-8999-999999999999\",\"payload\":{}}\n",
        "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"health.request\",\"id\":\"55555555-5555-4555-8555-555555555555\",\"payload\":{}}\n"
    );
    jsonl
        .stdin
        .take()
        .expect("jsonl stdin")
        .write_all(input.as_bytes())
        .expect("write jsonl");
    let jsonl_output = jsonl.wait_with_output().expect("wait jsonl");
    let jsonl_stdout = String::from_utf8(jsonl_output.stdout).expect("utf8");
    assert!(jsonl_output.status.success());
    assert_eq!(jsonl_stdout.lines().count(), 3);
    assert!(jsonl_stdout.contains("\"kind\":\"handshake.response\""));
    assert!(jsonl_stdout.contains("\"kind\":\"error.response\""));
    assert!(jsonl_stdout.contains("\"kind\":\"health.response\""));
    assert!(jsonl_stdout.contains("\"correlation_id\":\"11111111-1111-4111-8111-111111111111\""));
    assert!(jsonl_stdout.contains("\"correlation_id\":\"99999999-9999-4999-8999-999999999999\""));
    assert!(jsonl_stdout.contains("\"correlation_id\":\"55555555-5555-4555-8555-555555555555\""));
    assert!(jsonl_stdout.contains("\"error\":{\"code\":\"protocol.malformed\",\"message\":\"malformed request\",\"retryable\":false}"));
    assert!(jsonl_stdout.contains("\"id\":\"00000000-0000-4000-8000-"));
    assert!(jsonl_output.stderr.is_empty());

    let doctor = run(&["doctor"], &runtime);
    let doctor_stdout = String::from_utf8(doctor.stdout).expect("utf8");
    assert!(doctor.status.success());
    assert_eq!(doctor_stdout.lines().count(), 1);
    assert!(doctor_stdout.contains("\"status\":\"ok\""));
    assert!(doctor_stdout.contains("\"name\":\"health\""));
    assert!(doctor_stdout.contains("\"code\":null"));
    assert!(doctor.stderr.is_empty());

    let mut tui = Command::new(binary())
        .arg("tui")
        .env("XDG_RUNTIME_DIR", &runtime)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start tui");
    tui.stdin
        .take()
        .expect("tui stdin")
        .write_all(b"q\n")
        .expect("quit tui");
    let tui_output = tui.wait_with_output().expect("wait tui");
    let tui_stdout = String::from_utf8(tui_output.stdout).expect("utf8");
    assert!(tui_output.status.success());
    assert!(tui_stdout.contains("Akashic daemon"));
    assert!(tui_stdout.contains("Version: 0.1.0"));
    assert!(tui_stdout.contains("Health: ok"));
    assert!(!tui_stdout.contains("task"));
    assert!(tui_output.stderr.is_empty());

    let daemon_output = stop_daemon(daemon);
    assert!(daemon_output.stdout.is_empty());
}
