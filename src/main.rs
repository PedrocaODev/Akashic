use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(not(target_os = "linux"))]
compile_error!("Akashic bootstrap secure runtime is Linux-only; qualify non-Linux manually");

mod artifacts;
mod config;
mod json;
mod runtime;
mod shutdown;

use config::{Config, ConfigError, Overrides};
use json::Value;

const VERSION: &str = "0.1.0";
const PROTOCOL: &str = "akashic.local";
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let parsed = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(()) => std::process::exit(usage_error()),
    };
    if parsed.mode == Mode::Version {
        if let Err(error) = config::validate_overrides(&parsed.overrides) {
            std::process::exit(startup_error(error.code, error.message, 2));
        }
        std::process::exit(version());
    }
    let config = match config::resolve(&parsed.overrides) {
        Ok(config) => config,
        Err(error) if parsed.mode == Mode::Doctor => {
            std::process::exit(doctor_config_error(&error))
        }
        Err(error) => std::process::exit(startup_error(error.code, error.message, 2)),
    };
    std::process::exit(match parsed.mode {
        Mode::Version => version(),
        Mode::Daemon => daemon(&config),
        Mode::Tui => tui(),
        Mode::Doctor => doctor(&config),
        Mode::Jsonl => jsonl(&config),
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Version,
    Daemon,
    Tui,
    Doctor,
    Jsonl,
}

struct ParsedArgs {
    mode: Mode,
    overrides: Overrides,
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, ()> {
    let mut overrides = Overrides::default();
    let mut index = 0;
    while index < args.len() && args[index].starts_with("--") {
        let flag = args[index].as_str();
        if index + 1 >= args.len() || args[index + 1].starts_with("--") {
            return Err(());
        }
        let value = args[index + 1].clone();
        match flag {
            "--log-level" => overrides.log_level = Some(value),
            "--shutdown-timeout-seconds" => overrides.shutdown_timeout_seconds = Some(value),
            "--jsonl-max-line-bytes" => overrides.jsonl_max_line_bytes = Some(value),
            _ => return Err(()),
        }
        index += 2;
    }
    let remaining = &args[index..];
    let mode = match remaining {
        [command] if command == "version" => Mode::Version,
        [command] if command == "daemon" => Mode::Daemon,
        [command] if command == "tui" => Mode::Tui,
        [command] if command == "doctor" => Mode::Doctor,
        [command, flag] if command == "run" && flag == "--jsonl" => Mode::Jsonl,
        _ => return Err(()),
    };
    Ok(ParsedArgs { mode, overrides })
}

fn version() -> i32 {
    println!("akashic {VERSION}");
    0
}

fn usage_error() -> i32 {
    eprintln!("{}", error("usage.invalid", "invalid usage", false));
    2
}

fn error(code: &str, message: &str, retryable: bool) -> String {
    format!(
        "{{\"code\":{},\"message\":{},\"retryable\":{retryable}}}",
        json_quote(code),
        json_quote(message)
    )
}

fn json_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                quoted.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn startup_error(code: &str, message: &str, exit: i32) -> i32 {
    eprintln!(
        "{}",
        error(
            code,
            message,
            code.starts_with("lifecycle.") || code == "internal.unexpected"
        )
    );
    exit
}

fn startup_exit_for_code(code: &str) -> i32 {
    if code.starts_with("config.") {
        2
    } else if code.starts_with("protocol.") || code.starts_with("authorization.") {
        3
    } else if code == "lifecycle.shutdown_timeout" {
        124
    } else if code.starts_with("lifecycle.") {
        4
    } else {
        1
    }
}

fn daemon(config: &Config) -> i32 {
    if !shutdown::install_signal_handlers() {
        return startup_error("internal.unexpected", "signal setup failed", 1);
    }
    let mut runtime = match runtime::acquire() {
        Ok(runtime) => runtime,
        Err(error) => return runtime_startup_error(error),
    };
    let store_path = match runtime::artifact_store_path() {
        Ok(path) => path,
        Err(error) => return runtime_startup_error(error),
    };
    let _store = match artifacts::Store::open(&store_path) {
        Ok(store) => store,
        Err(error) => {
            return startup_error(
                error.code(),
                error.message(),
                startup_exit_for_code(error.code()),
            );
        }
    };
    let listener = match runtime.bind() {
        Ok(listener) => listener,
        Err(error) => return runtime_startup_error(error),
    };
    if listener.set_nonblocking(true).is_err() {
        return startup_error("internal.unexpected", "daemon listener unavailable", 1);
    }
    let daemon_id = new_id();
    let max_line_bytes = config.jsonl_max_line_bytes;
    let stopping = Arc::new(AtomicBool::new(false));
    let mut children: Vec<JoinHandle<()>> = Vec::new();
    loop {
        if shutdown::first_signal() != 0 {
            stopping.store(true, Ordering::SeqCst);
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if runtime::authorize_peer(&stream).is_err() {
                    let response = response_error(
                        &new_id(),
                        "authorization.peer_uid",
                        "peer authorization failed",
                    );
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }
                let stopping = Arc::clone(&stopping);
                let daemon_id = daemon_id.clone();
                children.push(thread::spawn(move || {
                    handle_client(stream, &daemon_id, max_line_bytes, stopping)
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                stopping.store(true, Ordering::SeqCst);
                let _ = runtime.cleanup();
                return startup_error("internal.unexpected", "daemon listener failed", 1);
            }
        }
    }
    drop(listener);
    let cleanup_signal_window = Instant::now() + Duration::from_millis(50);
    while Instant::now() < cleanup_signal_window {
        if let Some(exit) = shutdown::second_signal_exit(shutdown::second_signal()) {
            return exit;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let cleanup_failed = runtime.cleanup().is_err();
    if let Some(exit) = shutdown::second_signal_exit(shutdown::second_signal()) {
        return exit;
    }
    if cleanup_failed {
        return startup_error("internal.unexpected", "runtime cleanup failed", 1);
    }
    let deadline = Instant::now() + Duration::from_secs(config.shutdown_timeout_seconds);
    let second_signal_window = Instant::now() + Duration::from_millis(50);
    loop {
        if let Some(exit) = shutdown::second_signal_exit(shutdown::second_signal()) {
            return exit;
        }
        if Instant::now() < second_signal_window {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        if children.iter().all(JoinHandle::is_finished) {
            let failed = children.into_iter().any(|child| child.join().is_err());
            return if failed {
                startup_error(
                    "internal.unexpected",
                    "internal error",
                    shutdown::shutdown_exit(shutdown::ShutdownOutcome::ChildFailed),
                )
            } else if let Some(exit) = shutdown::second_signal_exit(shutdown::second_signal()) {
                exit
            } else {
                shutdown::shutdown_exit(shutdown::ShutdownOutcome::Clean)
            };
        }
        if Instant::now() >= deadline {
            let (code, message) = shutdown::shutdown_timeout_error();
            return startup_error(
                code,
                message,
                shutdown::shutdown_exit(shutdown::ShutdownOutcome::TimedOut),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn runtime_startup_error(error: runtime::RuntimeError) -> i32 {
    let exit = if error.code() == "lifecycle.daemon_running" {
        4
    } else {
        2
    };
    startup_error(error.code(), error.message(), exit)
}

enum Frame {
    End,
    Pending,
    Complete(Vec<u8>),
    Incomplete(Vec<u8>),
    InvalidUtf8 { terminal: bool },
    Oversized(Vec<u8>),
}

struct FrameReader<R> {
    reader: R,
    frame: Vec<u8>,
    discarding: bool,
}

impl<R: Read> FrameReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            frame: Vec::new(),
            discarding: false,
        }
    }

    fn next(&mut self, max_line_bytes: usize) -> io::Result<Frame> {
        loop {
            let mut byte = [0; 1];
            let read = match self.reader.read(&mut byte) {
                Ok(read) => read,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(Frame::Pending)
                }
                Err(error) => return Err(error),
            };
            if read == 0 {
                if self.discarding {
                    self.discarding = false;
                    return Ok(Frame::Oversized(std::mem::take(&mut self.frame)));
                }
                if self.frame.is_empty() {
                    return Ok(Frame::End);
                }
                let frame = std::mem::take(&mut self.frame);
                if std::str::from_utf8(&frame).is_ok() {
                    return Ok(Frame::Incomplete(frame));
                }
                return Ok(Frame::InvalidUtf8 { terminal: true });
            }
            if self.discarding {
                if byte[0] == b'\n' {
                    self.discarding = false;
                    return Ok(Frame::Oversized(std::mem::take(&mut self.frame)));
                }
                continue;
            }
            if self.frame.len() >= max_line_bytes {
                self.discarding = byte[0] != b'\n';
                if !self.discarding {
                    return Ok(Frame::Oversized(std::mem::take(&mut self.frame)));
                }
                continue;
            }
            self.frame.push(byte[0]);
            if byte[0] == b'\n' {
                let frame = std::mem::take(&mut self.frame);
                if std::str::from_utf8(&frame).is_ok() {
                    return Ok(Frame::Complete(frame));
                }
                return Ok(Frame::InvalidUtf8 { terminal: false });
            }
        }
    }
}

fn frame_error(frame: &Frame) -> String {
    let correlation = match frame {
        Frame::Incomplete(bytes) | Frame::Oversized(bytes) => correlation_prefix(bytes),
        _ => new_id(),
    };
    match frame {
        Frame::Oversized(_) => {
            response_error(&correlation, "protocol.oversized", "request too large")
        }
        Frame::Incomplete(_) | Frame::InvalidUtf8 { .. } => {
            response_error(&correlation, "protocol.malformed", "malformed request")
        }
        Frame::End | Frame::Pending | Frame::Complete(_) => String::new(),
    }
}

fn handle_client(
    stream: UnixStream,
    daemon_id: &str,
    max_line_bytes: usize,
    stopping: Arc<AtomicBool>,
) {
    if stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .is_err()
    {
        return;
    }
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut reader = FrameReader::new(BufReader::new(reader_stream));
    let mut writer = stream;
    let mut handshaken = false;
    loop {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let frame = match reader.next(max_line_bytes) {
            Ok(frame) => frame,
            Err(_) => return,
        };
        if matches!(frame, Frame::Pending) {
            continue;
        }
        let terminal = matches!(
            frame,
            Frame::Incomplete(_) | Frame::InvalidUtf8 { terminal: true }
        );
        if !matches!(frame, Frame::Complete(_)) {
            if matches!(frame, Frame::End) {
                return;
            }
            let response = frame_error(&frame);
            if writer.write_all(response.as_bytes()).is_err() || writer.flush().is_err() {
                return;
            }
            if terminal {
                return;
            }
            continue;
        }
        let Frame::Complete(line) = frame else {
            unreachable!()
        };
        let (response, completed_handshake) = handle_request(&line, handshaken, daemon_id);
        handshaken |= completed_handshake;
        if writer.write_all(response.as_bytes()).is_err() || writer.flush().is_err() {
            return;
        }
    }
}

fn handle_request(line: &[u8], handshaken: bool, daemon_id: &str) -> (String, bool) {
    let request = match std::str::from_utf8(line) {
        Ok(request) => request,
        Err(_) => {
            return (
                response_error(&new_id(), "protocol.malformed", "malformed request"),
                false,
            )
        }
    };
    let Some(envelope) = json::envelope(request) else {
        return (
            response_error(
                &correlation_prefix(line),
                "protocol.malformed",
                "malformed request",
            ),
            false,
        );
    };
    let correlation = envelope
        .id
        .clone()
        .filter(|id| valid_uuid(id))
        .unwrap_or_else(new_id);
    if !envelope.id.as_deref().is_some_and(valid_uuid) {
        return (
            response_error(&correlation, "protocol.malformed", "malformed request"),
            false,
        );
    }
    if (envelope.correlation_id_present && envelope.correlation_id.is_none())
        || envelope
            .correlation_id
            .as_deref()
            .is_some_and(|id| !valid_uuid(id))
    {
        return (
            response_error(&correlation, "protocol.malformed", "malformed request"),
            false,
        );
    }
    if !envelope.version_present || envelope.version.is_none() {
        return (
            response_error(&correlation, "protocol.malformed", "malformed request"),
            false,
        );
    }
    if envelope.protocol.as_deref() != Some(PROTOCOL) || envelope.version.as_deref() != Some("1") {
        return (
            response_error(
                &correlation,
                "protocol.unsupported_version",
                "unsupported protocol",
            ),
            false,
        );
    }
    if envelope.payload.is_some() == envelope.error.is_some() {
        return (
            response_error(&correlation, "protocol.malformed", "malformed request"),
            false,
        );
    }
    match envelope.kind.as_deref() {
        Some("handshake.request") if !handshaken => {
            let complete = envelope.payload.as_ref().is_some_and(|payload| {
                if !object_has_exact_fields(
                    payload,
                    &["client_role", "client_version", "client_instance_id"],
                ) {
                    return false;
                }
                let Some(role) = json::object_field(payload, "client_role").and_then(json::string_value) else {
                    return false;
                };
                matches!(role, "tui" | "jsonl" | "doctor")
                    && json::object_field(payload, "client_version")
                        .and_then(json::string_value)
                        .is_some_and(valid_semver)
                    && json::object_field(payload, "client_instance_id")
                        .and_then(json::string_value)
                        .is_some_and(valid_uuid)
            });
            if !complete {
                return (
                    response_error(&correlation, "protocol.malformed", "malformed request"),
                    false,
                );
            }
            (
                format!(
                    "{{\"protocol\":\"{PROTOCOL}\",\"version\":1,\"kind\":\"handshake.response\",\"id\":\"{}\",\"correlation_id\":{},\"payload\":{{\"daemon_version\":\"{VERSION}\",\"daemon_instance_id\":{},\"protocol_version\":1,\"capabilities\":[\"health\"]}}}}\n",
                    new_id(),
                    json_quote(&correlation),
                    json_quote(daemon_id)
                ),
                true,
            )
        }
        Some("health.request")
            if handshaken
                && matches!(envelope.payload.as_ref(), Some(Value::Object(fields)) if fields.is_empty()) =>
        (
            format!(
                "{{\"protocol\":\"{PROTOCOL}\",\"version\":1,\"kind\":\"health.response\",\"id\":\"{}\",\"correlation_id\":{},\"payload\":{{\"status\":\"ok\",\"daemon_version\":\"{VERSION}\",\"daemon_instance_id\":{},\"protocol_version\":1}}}}\n",
                new_id(),
                json_quote(&correlation),
                json_quote(daemon_id)
            ),
            false,
        ),
        _ => (response_error(&correlation, "protocol.malformed", "malformed request"), false),
    }
}

fn response_error(correlation: &str, code: &str, message: &str) -> String {
    format!(
        "{{\"protocol\":\"{PROTOCOL}\",\"version\":1,\"kind\":\"error.response\",\"id\":\"{}\",\"correlation_id\":{},\"error\":{}}}\n",
        new_id(),
        json_quote(correlation),
        error(code, message, false)
    )
}

fn new_id() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn valid_semver(value: &str) -> bool {
    let (core, build) = value
        .split_once('+')
        .map_or((value, None), |(core, build)| (core, Some(build)));
    if build.is_some_and(|build| {
        build.is_empty()
            || build.split('.').any(|part| {
                part.is_empty()
                    || !part
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
            })
    }) {
        return false;
    }
    let (core, prerelease) = core
        .split_once('-')
        .map_or((core, None), |(core, prerelease)| (core, Some(prerelease)));
    let core_parts = core.split('.').collect::<Vec<_>>();
    if core_parts.len() != 3
        || core_parts.iter().any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.chars().all(|character| character.is_ascii_digit())
        })
    {
        return false;
    }
    prerelease.is_none_or(|prerelease| {
        !prerelease.is_empty()
            && prerelease.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
                    && !(part.len() > 1
                        && part.chars().all(|character| character.is_ascii_digit())
                        && part.starts_with('0'))
            })
    })
}

fn correlation_prefix(input: &[u8]) -> String {
    std::str::from_utf8(input)
        .ok()
        .and_then(|input| json::top_level_string_prefix(input, "id"))
        .filter(|id| valid_uuid(id))
        .unwrap_or_else(new_id)
}

fn response_error_code(response: &str, correlation_id: &str) -> Option<&'static str> {
    let envelope = json::envelope(response)?;
    if envelope.protocol.as_deref() != Some(PROTOCOL)
        || envelope.version.as_deref() != Some("1")
        || envelope.kind.as_deref() != Some("error.response")
        || envelope.payload.is_some()
        || !envelope.id.as_deref().is_some_and(valid_uuid)
        || envelope.correlation_id.as_deref() != Some(correlation_id)
    {
        return None;
    }
    let Value::Object(error) = envelope.error? else {
        return None;
    };
    if error.len() != 3
        || error
            .iter()
            .any(|(key, _)| !matches!(key.as_str(), "code" | "message" | "retryable"))
    {
        return None;
    }
    let code = error
        .iter()
        .find(|(key, _)| key == "code")
        .and_then(|(_, value)| json::string_value(value))?;
    let _message = error
        .iter()
        .find(|(key, _)| key == "message")
        .and_then(|(_, value)| json::string_value(value))?;
    let retryable = error
        .iter()
        .find(|(key, _)| key == "retryable")
        .and_then(|(_, value)| match value {
            Value::Bool(value) => Some(*value),
            _ => None,
        })?;
    if locked_retryable(code) != Some(retryable) {
        return None;
    }
    match code {
        "usage.invalid" => Some("usage.invalid"),
        "config.invalid" => Some("config.invalid"),
        "config.unsupported_version" => Some("config.unsupported_version"),
        "config.secret_forbidden" => Some("config.secret_forbidden"),
        "protocol.malformed" => Some("protocol.malformed"),
        "protocol.unsupported_version" => Some("protocol.unsupported_version"),
        "protocol.oversized" => Some("protocol.oversized"),
        "authorization.peer_uid" => Some("authorization.peer_uid"),
        "lifecycle.daemon_running" => Some("lifecycle.daemon_running"),
        "lifecycle.daemon_unavailable" => Some("lifecycle.daemon_unavailable"),
        "lifecycle.shutdown_timeout" => Some("lifecycle.shutdown_timeout"),
        "internal.unexpected" => Some("internal.unexpected"),
        _ => None,
    }
}

fn locked_retryable(code: &str) -> Option<bool> {
    if code == "usage.invalid"
        || code.starts_with("config.")
        || code.starts_with("protocol.")
        || code.starts_with("authorization.")
    {
        Some(false)
    } else if code.starts_with("lifecycle.") || code == "internal.unexpected" {
        Some(true)
    } else {
        None
    }
}

fn response_matches(response: &str, expected_kind: &str, correlation_id: &str) -> bool {
    let Some(envelope) = json::envelope(response) else {
        return false;
    };
    if envelope.protocol.as_deref() != Some(PROTOCOL)
        || envelope.version.as_deref() != Some("1")
        || envelope.kind.as_deref() != Some(expected_kind)
        || !envelope.id.as_deref().is_some_and(valid_uuid)
        || envelope.correlation_id.as_deref() != Some(correlation_id)
        || envelope.payload.is_none()
        || envelope.error.is_some()
    {
        return false;
    }
    let Some(Value::Object(payload)) = envelope.payload else {
        return false;
    };
    let expected_fields = match expected_kind {
        "handshake.response" => [
            "daemon_version",
            "daemon_instance_id",
            "protocol_version",
            "capabilities",
        ]
        .as_slice(),
        "health.response" => [
            "status",
            "daemon_version",
            "daemon_instance_id",
            "protocol_version",
        ]
        .as_slice(),
        _ => return false,
    };
    if payload.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !payload.iter().any(|(key, _)| key == field))
    {
        return false;
    }
    let Some(daemon_version) = payload
        .iter()
        .find(|(key, _)| key == "daemon_version")
        .and_then(|(_, value)| json::string_value(value))
    else {
        return false;
    };
    if !valid_semver(daemon_version)
        || payload
            .iter()
            .find(|(key, _)| key == "daemon_instance_id")
            .and_then(|(_, value)| json::string_value(value))
            .is_none_or(|id| !valid_uuid(id))
        || !payload
            .iter()
            .any(|(key, value)| key == "protocol_version" && json::number_is(value, "1"))
    {
        return false;
    }
    match expected_kind {
        "handshake.response" => payload.iter().any(|(key, value)| {
            key == "capabilities"
                && matches!(value, Value::Array(values) if values.len() == 1 && matches!(&values[0], Value::String(value) if value == "health"))
        }),
        "health.response" => payload.iter().any(|(key, value)| {
            key == "status" && json::string_value(value) == Some("ok")
        }),
        _ => false,
    }
}

fn object_has_exact_fields(value: &Value, expected: &[&str]) -> bool {
    let Value::Object(fields) = value else {
        return false;
    };
    fields.len() == expected.len()
        && expected
            .iter()
            .all(|field| fields.iter().any(|(key, _)| key == field))
}

fn protocol_failure_message(code: &str) -> &'static str {
    match code {
        "authorization.peer_uid" => "peer authorization failed",
        "protocol.unsupported_version" => "unsupported protocol",
        "protocol.oversized" => "request too large",
        _ => "protocol failure",
    }
}

fn doctor_protocol_check(
    name: &str,
    response: &str,
    expected_kind: &str,
    correlation_id: &str,
) -> Option<String> {
    if response_matches(response, expected_kind, correlation_id) {
        return None;
    }
    let code = response_error_code(response, correlation_id).unwrap_or("protocol.malformed");
    Some(check(
        name,
        "error",
        Some(code),
        protocol_failure_message(code),
    ))
}

fn doctor_error(check: String) -> (&'static str, i32, String) {
    ("error", 1, check)
}

fn connect() -> Result<UnixStream, i32> {
    let paths = runtime::probe().map_err(|_| 4)?;
    UnixStream::connect(paths.socket).map_err(|_| 4)
}

fn handshake(role: &str) -> (String, String) {
    let request_id = new_id();
    (
        format!(
        "{{\"protocol\":\"{PROTOCOL}\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"{}\",\"payload\":{{\"client_role\":\"{role}\",\"client_version\":\"{VERSION}\",\"client_instance_id\":\"{}\"}}}}\n",
        request_id,
        new_id()
        ),
        request_id,
    )
}

fn health_request() -> (String, String) {
    let request_id = new_id();
    (
        format!(
            "{{\"protocol\":\"{PROTOCOL}\",\"version\":1,\"kind\":\"health.request\",\"id\":\"{request_id}\",\"payload\":{{}}}}\n"
        ),
        request_id,
    )
}

fn request(
    stream: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    body: &str,
) -> io::Result<String> {
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    let mut response = String::new();
    reader.read_line(&mut response)?;
    Ok(response)
}

fn jsonl(config: &Config) -> i32 {
    let stream = match connect() {
        Ok(stream) => stream,
        Err(exit) => {
            return startup_error("lifecycle.daemon_unavailable", "daemon unavailable", exit)
        }
    };
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return startup_error("internal.unexpected", "internal error", 1),
    };
    let mut stream = stream;
    let mut reader = BufReader::new(reader_stream);
    let stdin = io::stdin();
    let mut input = FrameReader::new(BufReader::new(stdin.lock()));
    loop {
        let frame = match input.next(config.jsonl_max_line_bytes) {
            Ok(frame) => frame,
            Err(_) => return 3,
        };
        match frame {
            Frame::End => break,
            Frame::Pending => continue,
            frame @ Frame::Oversized(_) => {
                println!("{}", frame_error(&frame).trim_end());
            }
            Frame::Complete(line) => match std::str::from_utf8(&line) {
                Ok(line) => match request(&mut stream, &mut reader, line) {
                    Ok(response) => print!("{response}"),
                    Err(_) => return 3,
                },
                Err(_) => println!(
                    "{}",
                    frame_error(&Frame::InvalidUtf8 { terminal: false }).trim_end()
                ),
            },
            frame @ (Frame::Incomplete(_) | Frame::InvalidUtf8 { .. }) => {
                let terminal = matches!(
                    frame,
                    Frame::Incomplete(_) | Frame::InvalidUtf8 { terminal: true }
                );
                println!("{}", frame_error(&frame).trim_end());
                if terminal {
                    break;
                }
            }
        }
        let _ = io::stdout().flush();
    }
    0
}

fn tui() -> i32 {
    let mut stream = match connect() {
        Ok(stream) => stream,
        Err(exit) => {
            return startup_error("lifecycle.daemon_unavailable", "daemon unavailable", exit)
        }
    };
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return startup_error("internal.unexpected", "internal error", 1),
    };
    let mut reader = BufReader::new(reader_stream);
    let (handshake_request, handshake_id) = handshake("tui");
    let handshake_response = match request(&mut stream, &mut reader, &handshake_request) {
        Ok(response) => response,
        Err(_) => return startup_error("protocol.malformed", "protocol handshake failed", 3),
    };
    if !response_matches(&handshake_response, "handshake.response", &handshake_id) {
        let code =
            response_error_code(&handshake_response, &handshake_id).unwrap_or("protocol.malformed");
        return startup_error(
            code,
            "protocol handshake failed",
            startup_exit_for_code(code),
        );
    }
    let (health_request, health_id) = health_request();
    let health_response = match request(&mut stream, &mut reader, &health_request) {
        Ok(response) => response,
        Err(_) => return startup_error("protocol.malformed", "protocol health failed", 3),
    };
    if !response_matches(&health_response, "health.response", &health_id) {
        let code =
            response_error_code(&health_response, &health_id).unwrap_or("protocol.malformed");
        return startup_error(code, "protocol health failed", startup_exit_for_code(code));
    }
    println!("Akashic daemon");
    println!("Version: {VERSION}");
    println!("Health: ok");
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let mut line = String::new();
    while input.read_line(&mut line).is_ok() {
        if matches!(line.trim(), "q" | "quit" | "c" | "cancel") {
            break;
        }
        if line.is_empty() {
            break;
        }
        line.clear();
    }
    0
}

fn check(name: &str, status: &str, code: Option<&str>, message: &str) -> String {
    let code = code.map_or_else(|| "null".to_string(), json_quote);
    format!(
        "{{\"name\":{},\"status\":{},\"code\":{},\"message\":{}}}",
        json_quote(name),
        json_quote(status),
        code,
        json_quote(message)
    )
}

fn doctor_config_error(error: &ConfigError) -> i32 {
    doctor_result(
        "error",
        &check("config", "error", Some(error.code), error.message),
    );
    1
}

fn doctor_result(status: &str, checks: &str) {
    println!(
        "{{\"status\":\"{status}\",\"version\":\"{VERSION}\",\"protocol\":{{\"identifier\":\"{PROTOCOL}\",\"version\":1}},\"checks\":[{checks}]}}"
    );
}

fn doctor(config: &Config) -> i32 {
    let result = match runtime::probe() {
        Err(error) => (
            "error",
            1,
            check("runtime", "error", Some(error.code()), error.message()),
        ),
        Ok(paths) => {
            let socket = paths.socket;
            let result = if !socket.exists() {
                (
                    "degraded",
                    4,
                    format!(
                        ",{}",
                        check(
                            "daemon",
                            "warning",
                            Some("lifecycle.daemon_unavailable"),
                            "daemon unavailable",
                        )
                    ),
                )
            } else if !socket
                .symlink_metadata()
                .map(|m| m.file_type().is_socket())
                .unwrap_or(false)
            {
                (
                    "error",
                    1,
                    check(
                        "socket",
                        "error",
                        Some("protocol.malformed"),
                        "daemon socket is not a socket",
                    ),
                )
            } else {
                match connect() {
                    Ok(mut stream) => {
                        let reader_stream = stream.try_clone();
                        if let Ok(reader_stream) = reader_stream {
                            let mut reader = BufReader::new(reader_stream);
                            let (handshake_request, handshake_id) = handshake("doctor");
                            let handshake_result =
                                request(&mut stream, &mut reader, &handshake_request);
                            if let Ok(handshake_response) = handshake_result {
                                if let Some(failure) = doctor_protocol_check(
                                    "handshake",
                                    &handshake_response,
                                    "handshake.response",
                                    &handshake_id,
                                ) {
                                    doctor_error(failure)
                                } else {
                                    let (health, health_id) = health_request();
                                    match request(&mut stream, &mut reader, &health) {
                                        Ok(health_response) => {
                                            if let Some(failure) = doctor_protocol_check(
                                                "health",
                                                &health_response,
                                                "health.response",
                                                &health_id,
                                            ) {
                                                doctor_error(failure)
                                            } else {
                                                (
                                                    "ok",
                                                    0,
                                                    format!(
                                                        ",{},{}",
                                                        check(
                                                            "daemon",
                                                            "ok",
                                                            None,
                                                            "daemon available"
                                                        ),
                                                        check("health", "ok", None, "health ok")
                                                    ),
                                                )
                                            }
                                        }
                                        Err(_) => doctor_error(check(
                                            "health",
                                            "error",
                                            Some("protocol.malformed"),
                                            "protocol failure",
                                        )),
                                    }
                                }
                            } else {
                                doctor_error(check(
                                    "handshake",
                                    "error",
                                    Some("protocol.malformed"),
                                    "protocol failure",
                                ))
                            }
                        } else {
                            (
                                "error",
                                1,
                                check(
                                    "socket",
                                    "error",
                                    Some("protocol.malformed"),
                                    "socket unavailable",
                                ),
                            )
                        }
                    }
                    Err(_) => (
                        "error",
                        1,
                        check(
                            "socket",
                            "error",
                            Some("protocol.malformed"),
                            "socket unavailable",
                        ),
                    ),
                }
            };
            result
        }
    };
    let checks = format!(
        "{},{}",
        check("config", "ok", None, &config.summary()),
        result.2.trim_start_matches(',')
    );
    doctor_result(result.0, &checks);
    result.1
}

#[cfg(test)]
mod protocol_contract_tests {
    use super::*;

    fn fields_exact(value: &Value, expected: &[&str]) -> bool {
        let Value::Object(fields) = value else {
            return false;
        };
        fields.len() == expected.len()
            && expected
                .iter()
                .all(|field| fields.iter().any(|(key, _)| key == field))
    }

    fn parsed_object(response: &str) -> Vec<(String, Value)> {
        let Value::Object(fields) = json::parse(response).expect("valid response JSON") else {
            panic!("response is not an object");
        };
        fields
    }

    #[test]
    fn canonical_responses_have_structural_fields_types_ids_and_correlations() {
        let handshake = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{\"client_role\":\"jsonl\",\"client_version\":\"0.1.0\",\"client_instance_id\":\"22222222-2222-4222-8222-222222222222\"}}\n";
        let (handshake_response, completed) = handle_request(
            handshake.as_bytes(),
            false,
            "00000000-0000-4000-8000-000000000001",
        );
        assert!(completed);
        let handshake_fields = parsed_object(&handshake_response);
        assert!(fields_exact(
            &Value::Object(handshake_fields.clone()),
            &[
                "protocol",
                "version",
                "kind",
                "id",
                "correlation_id",
                "payload"
            ]
        ));
        let handshake_envelope = json::envelope(&handshake_response).expect("handshake envelope");
        assert_eq!(handshake_envelope.protocol.as_deref(), Some(PROTOCOL));
        assert_eq!(handshake_envelope.version.as_deref(), Some("1"));
        assert_eq!(
            handshake_envelope.kind.as_deref(),
            Some("handshake.response")
        );
        assert!(handshake_envelope.id.as_deref().is_some_and(valid_uuid));
        assert_eq!(
            handshake_envelope.correlation_id.as_deref(),
            Some("11111111-1111-4111-8111-111111111111")
        );
        let payload = handshake_envelope.payload.as_ref().expect("payload");
        assert!(fields_exact(
            payload,
            &[
                "daemon_version",
                "daemon_instance_id",
                "protocol_version",
                "capabilities"
            ]
        ));
        assert!(json::object_field(payload, "daemon_version")
            .and_then(json::string_value)
            .is_some_and(valid_semver));
        assert!(json::object_field(payload, "daemon_instance_id")
            .and_then(json::string_value)
            .is_some_and(valid_uuid));
        assert!(json::object_field(payload, "protocol_version")
            .is_some_and(|value| json::number_is(value, "1")));

        let health = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"health.request\",\"id\":\"55555555-5555-4555-8555-555555555555\",\"payload\":{}}\n";
        let (health_response, _) = handle_request(
            health.as_bytes(),
            true,
            "00000000-0000-4000-8000-000000000001",
        );
        let health_envelope = json::envelope(&health_response).expect("health envelope");
        assert_eq!(health_envelope.kind.as_deref(), Some("health.response"));
        assert!(health_envelope.id.as_deref().is_some_and(valid_uuid));
        assert_ne!(
            health_envelope.id.as_deref(),
            handshake_envelope.id.as_deref()
        );
        assert_eq!(
            health_envelope.correlation_id.as_deref(),
            Some("55555555-5555-4555-8555-555555555555")
        );
        assert!(fields_exact(
            health_envelope.payload.as_ref().expect("health payload"),
            &[
                "status",
                "daemon_version",
                "daemon_instance_id",
                "protocol_version"
            ]
        ));

        let (error_response, _) =
            handle_request(b"not JSON\n", false, "00000000-0000-4000-8000-000000000001");
        let error_fields = parsed_object(&error_response);
        assert!(fields_exact(
            &Value::Object(error_fields),
            &[
                "protocol",
                "version",
                "kind",
                "id",
                "correlation_id",
                "error"
            ]
        ));
        let error_envelope = json::envelope(&error_response).expect("error envelope");
        let error = error_envelope.error.as_ref().expect("error object");
        assert!(fields_exact(error, &["code", "message", "retryable"]));
        assert_eq!(
            json::object_field(error, "code").and_then(json::string_value),
            Some("protocol.malformed")
        );
        assert!(matches!(
            json::object_field(error, "retryable"),
            Some(Value::Bool(false))
        ));
        assert!(error_envelope.id.as_deref().is_some_and(valid_uuid));
        assert!(error_envelope
            .correlation_id
            .as_deref()
            .is_some_and(valid_uuid));
        assert!(json::envelope(&health_response).is_some());
    }
}
