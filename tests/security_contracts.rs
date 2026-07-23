use std::fs;
use std::io::{BufRead, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
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
        .join(format!("akashic-security-{stamp}"));
    fs::create_dir_all(root.join("config/akashic")).expect("config");
    fs::create_dir_all(root.join("home")).expect("home");
    root
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

fn socket(root: &Path) -> PathBuf {
    root.join("runtime/akashic/daemon.sock")
}

fn lock(root: &Path) -> PathBuf {
    root.join("runtime/akashic/daemon.lock")
}

fn start(root: &Path, xdg_runtime: &Path) -> Child {
    fs::create_dir_all(xdg_runtime).expect("runtime");
    fs::set_permissions(xdg_runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    let mut child = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .arg("daemon")
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", xdg_runtime)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    for _ in 0..100 {
        if UnixStream::connect(xdg_runtime.join("akashic/daemon.sock")).is_ok()
            && mode(&xdg_runtime.join("akashic/daemon.sock")) == 0o600
        {
            return child;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("daemon socket did not appear");
}

fn stop(mut daemon: Child) -> Output {
    daemon.kill().expect("stop daemon");
    daemon.wait_with_output().expect("daemon output")
}

fn daemon_output(root: &Path, xdg_runtime: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .arg("daemon")
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", xdg_runtime)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    for _ in 0..100 {
        if child.try_wait().expect("poll daemon").is_some() {
            return child.wait_with_output().expect("daemon output");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    child.wait_with_output().expect("daemon output")
}

fn daemon_output_with_state(root: &Path, xdg_runtime: &Path, state: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .arg("daemon")
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", xdg_runtime)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    for _ in 0..100 {
        if child.try_wait().expect("poll daemon").is_some() {
            return child.wait_with_output().expect("daemon output");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    child.wait_with_output().expect("daemon output")
}

#[test]
fn safe_runtime_uses_xdg_and_creates_private_paths() {
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    assert!(socket(&root).exists());
    assert!(lock(&root).exists());
    assert_eq!(mode(&runtime.join("akashic")), 0o700);
    assert_eq!(mode(&socket(&root)), 0o600);
    assert_eq!(mode(&lock(&root)), 0o600);
    let _ = stop(daemon);
}

#[test]
fn root_owned_readable_executable_ancestor_is_accepted_when_available() {
    let ancestor = match fs::metadata("/home") {
        Ok(metadata) => metadata,
        Err(_) => return,
    };
    if ancestor.uid() != 0 || ancestor.permissions().mode() & 0o022 != 0 {
        return;
    }
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    assert!(socket(&root).exists());
    let output = stop(daemon);
    assert!(output.stdout.is_empty());
}

#[test]
fn unsafe_xdg_runtime_falls_back_to_private_state_path() {
    let root = root();
    let runtime_file = root.join("runtime-file");
    fs::write(&runtime_file, b"not a directory").expect("runtime file");
    let state = root.join("state");
    fs::create_dir_all(&state).expect("state");
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).expect("state mode");
    let mut child = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .arg("daemon")
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", &runtime_file)
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon");
    let fallback_socket = state.join("akashic/run/daemon.sock");
    for _ in 0..100 {
        if UnixStream::connect(&fallback_socket).is_ok() {
            let output = stop(child);
            assert!(output.stderr.is_empty());
            assert_eq!(mode(&state.join("akashic/run")), 0o700);
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("fallback socket did not appear");
}

#[test]
fn unsafe_ancestor_is_rejected_without_binding() {
    let root = root();
    let unsafe_parent = root.join("unsafe-parent");
    fs::create_dir_all(&unsafe_parent).expect("unsafe parent");
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777)).expect("unsafe mode");
    let runtime = unsafe_parent.join("runtime");
    let output = daemon_output(&root, &runtime);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!runtime.join("akashic/daemon.sock").exists());
}

#[test]
fn symlinked_runtime_child_is_rejected_without_following_or_unlinking() {
    let root = root();
    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    let target = root.join("target-child");
    fs::create_dir(&target).expect("target child");
    std::os::unix::fs::symlink(&target, runtime.join("akashic")).expect("child symlink");
    let output = daemon_output(&root, &runtime);
    assert!(!output.status.success());
    assert!(fs::symlink_metadata(runtime.join("akashic"))
        .expect("symlink metadata")
        .file_type()
        .is_symlink());
    assert!(!target.join("daemon.sock").exists());
}

#[test]
fn active_lock_wins_before_socket_inspection_and_active_conflict_is_stable() {
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    let before = fs::metadata(socket(&root)).expect("socket metadata");
    let output = daemon_output(&root, &runtime);
    let after = fs::metadata(socket(&root)).expect("socket metadata");
    assert_eq!(output.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("lifecycle.daemon_running"),
        "status={:?} stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(before.ino(), after.ino());
    let _ = stop(daemon);
}

#[test]
fn verified_stale_socket_is_replaced_and_lock_is_released_on_exit() {
    let root = root();
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("runtime child");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let stale = UnixListener::bind(socket(&root)).expect("stale socket");
    fs::set_permissions(socket(&root), fs::Permissions::from_mode(0o600)).expect("stale mode");
    let stale_inode = fs::metadata(socket(&root)).expect("stale metadata").ino();
    drop(stale);
    let output = daemon_output(&root, &runtime);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.invalid"));
    assert_eq!(
        fs::metadata(socket(&root)).expect("stale metadata").ino(),
        stale_inode
    );
}

#[test]
fn own_peer_can_handshake_but_untrusted_socket_clients_are_checked_before_parsing() {
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    let mut client = UnixStream::connect(socket(&root)).expect("client");
    client.write_all(b"not json\n").expect("client request");
    let mut response = String::new();
    BufRead::read_line(&mut std::io::BufReader::new(client), &mut response).expect("response");
    assert!(response.contains("protocol.malformed"));
    let _ = stop(daemon);
}

#[test]
fn unauthorized_peer_is_rejected_before_request_parsing_when_linux_tools_exist() {
    if Command::new("setpriv").arg("--version").output().is_err()
        || Command::new("python3").arg("--version").output().is_err()
    {
        return;
    }
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).expect("runtime test mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o755))
        .expect("runtime child test mode");
    fs::set_permissions(socket(&root), fs::Permissions::from_mode(0o666))
        .expect("socket test mode");
    let script = "import socket,sys; s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.sendall(b'not json\\n'); print(s.recv(4096).decode())";
    let result = Command::new("setpriv")
        .args([
            "--reuid=65534",
            "--regid=65534",
            "--clear-groups",
            "python3",
            "-c",
            script,
            socket(&root).to_str().expect("socket path"),
        ])
        .output()
        .expect("unauthorized peer");
    let _ = stop(daemon);
    if !result.status.success()
        && String::from_utf8_lossy(&result.stderr).contains("Operation not permitted")
    {
        return;
    }
    assert!(
        result.status.success(),
        "status={:?} stdout={:?} stderr={:?}",
        result.status,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("authorization.peer_uid"));
}

#[test]
fn lock_is_released_after_graceful_shutdown() {
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    assert!(Command::new("kill")
        .args(["-TERM", &daemon.id().to_string()])
        .status()
        .expect("signal")
        .success());
    let output = daemon.wait_with_output().expect("daemon output");
    assert_eq!(output.status.code(), Some(0), "stderr={:?}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "shutdown stderr={:?}",
        output.stderr
    );
    let second = start(&root, &runtime);
    assert!(Command::new("kill")
        .args(["-TERM", &second.id().to_string()])
        .status()
        .expect("signal")
        .success());
    let second_output = second.wait_with_output().expect("daemon output");
    assert_eq!(second_output.status.code(), Some(0));
}

#[test]
fn special_permission_bits_are_rejected_on_runtime_directories() {
    for special_mode in [0o4700, 0o2700, 0o1700] {
        let root = root();
        let runtime = root.join("runtime");
        fs::create_dir(&runtime).expect("runtime");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(special_mode))
            .expect("runtime mode");
        let state = root.join("state-file");
        fs::write(&state, b"invalid state").expect("state file");
        let output = daemon_output_with_state(&root, &runtime, &state);
        assert!(!output.status.success(), "mode {special_mode:o}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("config.invalid"),
            "mode {special_mode:o}: {:?}",
            output.stderr
        );
        assert!(!runtime.join("akashic/daemon.sock").exists());
    }
}

#[test]
fn special_permission_bits_are_rejected_on_lock_and_stale_socket() {
    let first_root = root();
    let runtime = first_root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("runtime child");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    fs::write(lock(&first_root), b"").expect("lock");
    fs::set_permissions(lock(&first_root), fs::Permissions::from_mode(0o2600)).expect("lock mode");
    let output = daemon_output(&first_root, &runtime);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.invalid"));

    let second_root = root();
    let runtime = second_root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("runtime child");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let stale = UnixListener::bind(socket(&second_root)).expect("stale socket");
    fs::set_permissions(socket(&second_root), fs::Permissions::from_mode(0o1600))
        .expect("socket mode");
    drop(stale);
    let output = daemon_output(&second_root, &runtime);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.invalid"));
    assert!(socket(&second_root).exists());
}

#[test]
fn controlled_cleanup_failure_is_visible_and_replacement_is_preserved() {
    let root = root();
    let runtime = root.join("runtime");
    let daemon = start(&root, &runtime);
    fs::remove_file(socket(&root)).expect("remove original name");
    let replacement = UnixListener::bind(socket(&root)).expect("replacement socket");
    fs::set_permissions(socket(&root), fs::Permissions::from_mode(0o600))
        .expect("replacement mode");
    assert!(Command::new("kill")
        .args(["-TERM", &daemon.id().to_string()])
        .status()
        .expect("signal")
        .success());
    let output = daemon.wait_with_output().expect("daemon output");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("internal.unexpected"));
    assert!(socket(&root).exists());
    drop(replacement);
}

#[test]
fn parent_dir_components_are_rejected_before_descriptor_traversal() {
    let root = root();
    let runtime = root.join("runtime");
    fs::create_dir(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    let noncanonical = runtime.join("nested/../runtime");
    let output = daemon_output(&root, &noncanonical);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("config.invalid"));
    assert!(!root
        .join("runtime/nested/runtime/akashic/daemon.sock")
        .exists());
}
