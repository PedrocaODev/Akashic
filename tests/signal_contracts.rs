use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("akashic-signal-{stamp}"));
    fs::create_dir_all(root.join("config")).expect("config");
    fs::create_dir_all(root.join("home")).expect("home");
    fs::create_dir_all(root.join("runtime")).expect("runtime");
    fs::set_permissions(root.join("runtime"), fs::Permissions::from_mode(0o700))
        .expect("runtime mode");
    root
}

fn socket_path(root: &Path) -> PathBuf {
    root.join("runtime/akashic/daemon.sock")
}

fn start_daemon(root: &Path, timeout_seconds: u64) -> Child {
    fs::create_dir_all(root.join("config/akashic")).expect("akashic config");
    fs::write(
        root.join("config/akashic/config.toml"),
        format!("config_version = 1\nshutdown_timeout_seconds = {timeout_seconds}\n"),
    )
    .expect("config");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .arg("daemon")
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    for _ in 0..100 {
        if UnixStream::connect(socket_path(root)).is_ok() {
            return daemon;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
    panic!("daemon socket did not appear");
}

fn hold_client(root: &Path) -> UnixStream {
    UnixStream::connect(socket_path(root)).expect("client connection")
}

fn send_signal(pid: u32, signal: &str) {
    assert!(Command::new("kill")
        .args([format!("-{signal}"), pid.to_string()])
        .status()
        .expect("kill")
        .success());
}

fn wait_for_exit(mut daemon: Child) -> Output {
    for _ in 0..400 {
        if daemon.try_wait().expect("poll daemon").is_some() {
            return daemon.wait_with_output().expect("daemon output");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
    panic!("daemon did not shut down");
}

#[test]
fn first_sigint_and_sigterm_cancel_children_and_exit_cleanly() {
    for signal in ["INT", "TERM"] {
        let root = root();
        let daemon = start_daemon(&root, 1);
        let client = hold_client(&root);
        send_signal(daemon.id(), signal);
        let output = wait_for_exit(daemon);
        drop(client);
        assert_eq!(output.status.code(), Some(0), "SIG{signal}");
        assert!(output.stdout.is_empty(), "SIG{signal}");
        assert!(output.stderr.is_empty(), "SIG{signal}");
        assert!(!socket_path(&root).exists(), "SIG{signal}");
    }
}

#[test]
fn second_signals_use_their_own_exact_exit_codes_even_when_mixed() {
    for (first, second, expected) in [
        ("INT", "INT", 130),
        ("TERM", "TERM", 143),
        ("INT", "TERM", 143),
        ("TERM", "INT", 130),
    ] {
        let root = root();
        let daemon = start_daemon(&root, 1);
        let client = hold_client(&root);
        send_signal(daemon.id(), first);
        send_signal(daemon.id(), second);
        let output = wait_for_exit(daemon);
        drop(client);
        assert_eq!(output.status.code(), Some(expected), "{first}/{second}");
        assert!(output.stdout.is_empty(), "{first}/{second}");
        assert!(output.stderr.is_empty(), "{first}/{second}");
    }
}

#[test]
fn clean_shutdown_without_children_exits_zero() {
    let root = root();
    let daemon = start_daemon(&root, 1);
    send_signal(daemon.id(), "TERM");
    let output = wait_for_exit(daemon);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn second_signal_takes_precedence_over_controlled_cleanup_failure() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    for (first, second, expected) in [("INT", "TERM", 143), ("TERM", "INT", 130)] {
        let root = root();
        let daemon = start_daemon(&root, 1);
        fs::remove_file(socket_path(&root)).expect("remove daemon name");
        let replacement = UnixListener::bind(socket_path(&root)).expect("replacement socket");
        fs::set_permissions(socket_path(&root), fs::Permissions::from_mode(0o600))
            .expect("replacement mode");
        send_signal(daemon.id(), first);
        send_signal(daemon.id(), second);
        let output = wait_for_exit(daemon);
        drop(replacement);
        assert_eq!(output.status.code(), Some(expected), "{first}/{second}");
        assert!(output.stdout.is_empty(), "{first}/{second}");
        assert!(output.stderr.is_empty(), "{first}/{second}");
    }
}

#[allow(dead_code)]
fn drain_client(mut client: UnixStream) {
    let mut buffer = [0; 256];
    let _ = client.read(&mut buffer);
}
