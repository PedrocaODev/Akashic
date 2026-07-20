use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_akashic")
}

fn test_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("akashic-config-test-{stamp}"));
    fs::create_dir(&path).expect("test root");
    fs::write(path.join(".git"), b"test git root").expect("git marker");
    path
}

fn write_config(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("config parent")).expect("config directory");
    fs::write(path, contents).expect("config file");
}

fn doctor(root: &Path, args: &[&str]) -> Output {
    let runtime = root.join("runtime");
    let config_home = root.join("config");
    let home = root.join("home");
    fs::create_dir_all(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::create_dir_all(&config_home).expect("config home");
    fs::create_dir_all(&home).expect("home");
    let mut command = Command::new(binary());
    command
        .args(args)
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("HOME", &home)
        .env_remove("AKASHIC_LOG_LEVEL")
        .env_remove("AKASHIC_SHUTDOWN_TIMEOUT_SECONDS")
        .env_remove("AKASHIC_JSONL_MAX_LINE_BYTES");
    command.output().expect("run doctor")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf8")
}

#[test]
fn defaults_and_layered_paths_are_reported_with_cli_precedence() {
    let root = test_root();
    let nested = root.join("workspace/deep");
    fs::create_dir_all(&nested).expect("nested project");
    fs::write(root.join(".git"), b"gitdir").expect("git marker");
    write_config(
        &root.join("config/akashic/config.toml"),
        "config_version = 1\nlog_level = \"warn\"\nshutdown_timeout_seconds = 20\njsonl_max_line_bytes = 4096\n",
    );
    write_config(
        &root.join(".akashic/config.toml"),
        "config_version = 1\nlog_level = \"debug\"\nshutdown_timeout_seconds = 12\njsonl_max_line_bytes = 8192\n",
    );

    let mut command = Command::new(binary());
    command
        .args([
            "--log-level",
            "trace",
            "--shutdown-timeout-seconds",
            "6",
            "--jsonl-max-line-bytes",
            "16384",
            "doctor",
        ])
        .current_dir(&nested)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .env("AKASHIC_LOG_LEVEL", "error")
        .env("AKASHIC_SHUTDOWN_TIMEOUT_SECONDS", "8")
        .env("AKASHIC_JSONL_MAX_LINE_BYTES", "12288");
    let output = command.output().expect("run doctor");
    let body = stdout(&output);
    assert!(body.contains("log_level=trace"));
    assert!(body.contains("shutdown_timeout_seconds=6"));
    assert!(body.contains("jsonl_max_line_bytes=16384"));
}

#[test]
fn no_git_root_uses_current_directory_and_defaults_are_exact() {
    let root = test_root();
    write_config(
        &root.join(".akashic/config.toml"),
        "config_version = 1\nlog_level = \"info\"\n",
    );
    let output = doctor(&root, &["doctor"]);
    let body = stdout(&output);
    assert!(body.contains("log_level=info"));
    assert!(body.contains("shutdown_timeout_seconds=10"));
    assert!(body.contains("jsonl_max_line_bytes=1048576"));
}

#[test]
fn version_remains_exact_when_config_is_invalid() {
    let root = test_root();
    write_config(
        &root.join(".akashic/config.toml"),
        "config_version = 1\nUNKNOWN = \"ignored by version\"\n",
    );
    let output = doctor(&root, &["version"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"akashic 0.1.0\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn environment_overrides_project_and_user_values() {
    let root = test_root();
    write_config(
        &root.join("config/akashic/config.toml"),
        "config_version = 1\nlog_level = \"warn\"\nshutdown_timeout_seconds = 20\njsonl_max_line_bytes = 4096\n",
    );
    write_config(
        &root.join(".akashic/config.toml"),
        "config_version = 1\nlog_level = \"debug\"\nshutdown_timeout_seconds = 12\njsonl_max_line_bytes = 8192\n",
    );
    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).expect("runtime");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    let mut command = Command::new(binary());
    command
        .args(["doctor"])
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .env("AKASHIC_LOG_LEVEL", "trace")
        .env("AKASHIC_SHUTDOWN_TIMEOUT_SECONDS", "7")
        .env("AKASHIC_JSONL_MAX_LINE_BYTES", "2048");
    let body = stdout(&command.output().expect("run doctor"));
    assert!(body.contains("log_level=trace"));
    assert!(body.contains("shutdown_timeout_seconds=7"));
    assert!(body.contains("jsonl_max_line_bytes=2048"));
}

#[test]
fn cli_values_use_the_same_types_and_ranges() {
    let cases = [
        vec!["--log-level", "verbose", "doctor"],
        vec!["--shutdown-timeout-seconds", "0", "doctor"],
        vec!["--shutdown-timeout-seconds", "601", "doctor"],
        vec!["--jsonl-max-line-bytes", "1023", "doctor"],
        vec!["--jsonl-max-line-bytes", "1048577", "doctor"],
        vec!["--jsonl-max-line-bytes", "not-a-number", "doctor"],
    ];
    for args in cases {
        let root = test_root();
        let output = doctor(&root, &args);
        let body = stdout(&output);
        assert_eq!(body.lines().count(), 1);
        assert!(body.contains("\"status\":\"error\""));
        assert!(body.contains("\"code\":\"config.invalid\""));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn config_version_and_values_fail_closed() {
    let cases = [
        ("log_level = \"debug\"\n", "config.invalid"),
        ("config_version = 2\n", "config.unsupported_version"),
        (
            "config_version = 1\nlog_level = \"verbose\"\n",
            "config.invalid",
        ),
        (
            "config_version = 1\nshutdown_timeout_seconds = 0\n",
            "config.invalid",
        ),
        (
            "config_version = 1\nshutdown_timeout_seconds = 601\n",
            "config.invalid",
        ),
        (
            "config_version = 1\njsonl_max_line_bytes = 1023\n",
            "config.invalid",
        ),
        (
            "config_version = 1\njsonl_max_line_bytes = 1048577\n",
            "config.invalid",
        ),
        (
            "config_version = 1\nshutdown_timeout_seconds = \"ten\"\n",
            "config.invalid",
        ),
    ];
    for (contents, code) in cases {
        let root = test_root();
        write_config(&root.join(".akashic/config.toml"), contents);
        let output = doctor(&root, &["doctor"]);
        let body = stdout(&output);
        assert_eq!(body.lines().count(), 1);
        assert!(body.contains("\"status\":\"error\""));
        assert!(body.contains(&format!("\"code\":\"{code}\"")), "{body}");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn duplicate_config_keys_fail_closed() {
    let root = test_root();
    write_config(
        &root.join(".akashic/config.toml"),
        "config_version = 1\nlog_level = \"info\"\nlog_level = \"debug\"\n",
    );
    let output = doctor(&root, &["doctor"]);
    let body = stdout(&output);
    assert_eq!(body.lines().count(), 1);
    assert!(body.contains("\"status\":\"error\""));
    assert!(body.contains("\"code\":\"config.invalid\""));
    assert!(output.stderr.is_empty());
}

#[test]
fn unknown_and_secret_like_fields_are_rejected_without_leaking_values() {
    let cases = [
        (
            "config_version = 1\nunknown_key = \"value\"\n",
            "config.invalid",
            "value",
        ),
        (
            "config_version = 1\nApi_ToKeN = \"super-secret-token\"\n",
            "config.secret_forbidden",
            "super-secret-token",
        ),
        (
            "config_version = 1\nPASSWORD = \"hunter2\"\n",
            "config.secret_forbidden",
            "hunter2",
        ),
    ];
    for (contents, code, secret) in cases {
        let root = test_root();
        write_config(&root.join(".akashic/config.toml"), contents);
        let output = doctor(&root, &["doctor"]);
        let body = stdout(&output);
        assert!(body.contains(&format!("\"code\":\"{code}\"")));
        assert!(!body.contains(secret));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn non_doctor_config_errors_are_structured_stderr_with_exit_two() {
    let root = test_root();
    write_config(
        &root.join(".akashic/config.toml"),
        "config_version = 1\nBAD_TOKEN = \"do-not-leak\"\n",
    );
    let output = doctor(&root, &["tui"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"{\"code\":\"config.secret_forbidden\",\"message\":\"invalid configuration\",\"retryable\":false}\n"
    );
    assert!(!output
        .stderr
        .windows(b"do-not-leak".len())
        .any(|window| window == b"do-not-leak"));
}
