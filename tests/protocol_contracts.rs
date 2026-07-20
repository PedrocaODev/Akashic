use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROTOCOL: &str = "akashic.local";
const MAX_LINE: usize = 1_048_576;
const HANDSHAKE: &str = "{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"handshake.request\",\"id\":\"11111111-1111-4111-8111-111111111111\",\"payload\":{\"client_role\":\"jsonl\",\"client_version\":\"0.1.0\",\"client_instance_id\":\"22222222-2222-4222-8222-222222222222\"}}\n";

fn root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("akashic-protocol-{stamp}"));
    fs::create_dir_all(root.join("config/akashic")).expect("config");
    fs::create_dir_all(root.join("home")).expect("home");
    fs::create_dir_all(root.join("runtime")).expect("runtime");
    fs::set_permissions(root.join("runtime"), fs::Permissions::from_mode(0o700))
        .expect("runtime mode");
    root
}

fn socket(root: &Path) -> PathBuf {
    root.join("runtime/akashic/daemon.sock")
}

fn start(root: &Path) -> Child {
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
        if UnixStream::connect(socket(root)).is_ok() {
            return daemon;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
    panic!("daemon socket did not appear");
}

fn stop(mut daemon: Child) -> Output {
    daemon.kill().expect("stop daemon");
    daemon.wait_with_output().expect("daemon output")
}

fn request(stream: &mut UnixStream, body: &[u8]) -> String {
    let reader_stream = stream.try_clone().expect("reader");
    stream.write_all(body).expect("request");
    stream.flush().expect("flush");
    let mut reader = std::io::BufReader::new(reader_stream);
    let mut response = String::new();
    reader.read_line(&mut response).expect("response");
    response
}

fn jsonl(root: &Path, input: &[u8]) -> Output {
    let mut client = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .args(["run", "--jsonl"])
        .current_dir(root)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
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
    client.wait_with_output().expect("jsonl output")
}

fn health(id: &str) -> String {
    format!(
        "{{\"protocol\":\"{PROTOCOL}\",\"version\":1,\"kind\":\"health.request\",\"id\":\"{id}\",\"payload\":{{}}}}\n"
    )
}

#[derive(Debug, PartialEq)]
enum ParsedJson {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Object(BTreeMap<String, ParsedJson>),
    Array(Vec<ParsedJson>),
}

struct JsonParser<'a> {
    input: &'a [u8],
    position: usize,
}

fn parse_response(input: &str) -> ParsedJson {
    let mut parser = JsonParser {
        input: input.as_bytes(),
        position: 0,
    };
    let value = parser.value().expect("valid response JSON");
    parser.whitespace();
    assert_eq!(parser.position, parser.input.len());
    value
}

impl JsonParser<'_> {
    fn value(&mut self) -> Option<ParsedJson> {
        self.whitespace();
        match self.peek()? {
            b'n' => self.literal(b"null", ParsedJson::Null),
            b't' => self.literal(b"true", ParsedJson::Bool(true)),
            b'f' => self.literal(b"false", ParsedJson::Bool(false)),
            b'"' => self.string().map(ParsedJson::String),
            b'{' => self.object(),
            b'[' => self.array(),
            b'-' | b'0'..=b'9' => self.number().map(ParsedJson::Number),
            _ => None,
        }
    }

    fn object(&mut self) -> Option<ParsedJson> {
        self.consume(b'{')?;
        let mut fields = BTreeMap::new();
        self.whitespace();
        if self.consume(b'}').is_some() {
            return Some(ParsedJson::Object(fields));
        }
        loop {
            let key = self.string()?;
            self.whitespace();
            self.consume(b':')?;
            let value = self.value()?;
            if fields.insert(key, value).is_some() {
                return None;
            }
            self.whitespace();
            if self.consume(b'}').is_some() {
                return Some(ParsedJson::Object(fields));
            }
            self.consume(b',')?;
            self.whitespace();
            if self.peek() == Some(b'}') {
                return None;
            }
        }
    }

    fn array(&mut self) -> Option<ParsedJson> {
        self.consume(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.consume(b']').is_some() {
            return Some(ParsedJson::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.whitespace();
            if self.consume(b']').is_some() {
                return Some(ParsedJson::Array(values));
            }
            self.consume(b',')?;
            self.whitespace();
            if self.peek() == Some(b']') {
                return None;
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.consume(b'"')?;
        let mut output = String::new();
        while let Some(byte) = self.next() {
            match byte {
                b'"' => return Some(output),
                b'\\' => output.push(match self.next()? {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => return None,
                }),
                byte if byte.is_ascii() && !byte.is_ascii_control() => output.push(byte as char),
                _ => return None,
            }
        }
        None
    }

    fn number(&mut self) -> Option<String> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|byte| b"-+0123456789.eE".contains(&byte))
        {
            self.position += 1;
        }
        (self.position > start)
            .then(|| String::from_utf8(self.input[start..self.position].to_vec()).ok())
            .flatten()
    }

    fn literal(&mut self, literal: &[u8], value: ParsedJson) -> Option<ParsedJson> {
        self.input
            .get(self.position..)?
            .starts_with(literal)
            .then(|| {
                self.position += literal.len();
                value
            })
    }

    fn whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.position += 1;
        }
    }
    fn consume(&mut self, expected: u8) -> Option<()> {
        (self.peek()? == expected).then(|| {
            self.position += 1;
        })
    }
    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }
    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }
}

fn object(value: ParsedJson) -> BTreeMap<String, ParsedJson> {
    let ParsedJson::Object(value) = value else {
        panic!("expected object")
    };
    value
}

fn exact_keys(value: &BTreeMap<String, ParsedJson>, keys: &[&str]) {
    assert_eq!(value.len(), keys.len());
    for key in keys {
        assert!(value.contains_key(*key), "missing {key}");
    }
}

fn string_field<'a>(value: &'a BTreeMap<String, ParsedJson>, key: &str) -> &'a str {
    let ParsedJson::String(value) = value.get(key).expect("field") else {
        panic!("{key} not string")
    };
    value
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

fn assert_success_envelope(
    response: &str,
    kind: &str,
    request_id: &str,
) -> BTreeMap<String, ParsedJson> {
    let envelope = object(parse_response(response));
    exact_keys(
        &envelope,
        &[
            "protocol",
            "version",
            "kind",
            "id",
            "correlation_id",
            "payload",
        ],
    );
    assert_eq!(string_field(&envelope, "protocol"), PROTOCOL);
    assert!(matches!(envelope.get("version"), Some(ParsedJson::Number(value)) if value == "1"));
    assert_eq!(string_field(&envelope, "kind"), kind);
    assert!(valid_uuid(string_field(&envelope, "id")));
    assert_eq!(string_field(&envelope, "correlation_id"), request_id);
    envelope
}

fn assert_error_response(response: &str, request_id: &str, code: &str, retryable: bool) {
    let envelope = object(parse_response(response));
    exact_keys(
        &envelope,
        &[
            "protocol",
            "version",
            "kind",
            "id",
            "correlation_id",
            "error",
        ],
    );
    assert_eq!(string_field(&envelope, "protocol"), PROTOCOL);
    assert!(matches!(envelope.get("version"), Some(ParsedJson::Number(value)) if value == "1"));
    assert_eq!(string_field(&envelope, "kind"), "error.response");
    assert!(valid_uuid(string_field(&envelope, "id")));
    assert_eq!(string_field(&envelope, "correlation_id"), request_id);
    let ParsedJson::Object(error) = envelope.get("error").expect("error") else {
        panic!("error object")
    };
    exact_keys(error, &["code", "message", "retryable"]);
    assert_eq!(string_field(error, "code"), code);
    assert!(!string_field(error, "message").is_empty());
    assert_eq!(error.get("retryable"), Some(&ParsedJson::Bool(retryable)));
}

#[test]
fn handshake_and_health_have_exact_structured_envelopes_and_correlations() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let handshake = request(&mut stream, HANDSHAKE.as_bytes());
    let handshake_object = assert_success_envelope(
        &handshake,
        "handshake.response",
        "11111111-1111-4111-8111-111111111111",
    );
    let ParsedJson::Object(payload) = handshake_object.get("payload").expect("handshake payload")
    else {
        panic!("payload")
    };
    exact_keys(
        payload,
        &[
            "daemon_version",
            "daemon_instance_id",
            "protocol_version",
            "capabilities",
        ],
    );
    assert_eq!(string_field(payload, "daemon_version"), "0.1.0");
    assert!(valid_uuid(string_field(payload, "daemon_instance_id")));
    assert_eq!(
        payload.get("protocol_version"),
        Some(&ParsedJson::Number("1".to_string()))
    );
    assert_eq!(
        payload.get("capabilities"),
        Some(&ParsedJson::Array(vec![ParsedJson::String(
            "health".to_string()
        )]))
    );

    let response = request(
        &mut stream,
        health("55555555-5555-4555-8555-555555555555").as_bytes(),
    );
    let health_object = assert_success_envelope(
        &response,
        "health.response",
        "55555555-5555-4555-8555-555555555555",
    );
    let ParsedJson::Object(payload) = health_object.get("payload").expect("health payload") else {
        panic!("payload")
    };
    exact_keys(
        payload,
        &[
            "status",
            "daemon_version",
            "daemon_instance_id",
            "protocol_version",
        ],
    );
    assert_eq!(string_field(payload, "status"), "ok");
    assert_eq!(string_field(payload, "daemon_version"), "0.1.0");
    assert!(valid_uuid(string_field(payload, "daemon_instance_id")));
    assert_eq!(
        payload.get("protocol_version"),
        Some(&ParsedJson::Number("1".to_string()))
    );
    let output = stop(daemon);
    assert!(output.stdout.is_empty());
}

#[test]
fn health_before_handshake_and_invalid_requests_have_no_effect() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let before = request(
        &mut stream,
        health("55555555-5555-4555-8555-555555555555").as_bytes(),
    );
    assert_error_response(
        &before,
        "55555555-5555-4555-8555-555555555555",
        "protocol.malformed",
        false,
    );

    let invalid_role = HANDSHAKE.replace("\"jsonl\"", "\"worker\"");
    let invalid = request(&mut stream, invalid_role.as_bytes());
    assert!(invalid.contains("\"code\":\"protocol.malformed\""));
    let extra = HANDSHAKE.replace("\"payload\":", "\"extra\":1,\"payload\":");
    let extra_response = request(&mut stream, extra.as_bytes());
    assert_error_response(
        &extra_response,
        "11111111-1111-4111-8111-111111111111",
        "protocol.malformed",
        false,
    );
    let unsupported = request(
        &mut stream,
        HANDSHAKE.replace("akashic.local", "other.local").as_bytes(),
    );
    assert!(unsupported.contains("\"code\":\"protocol.unsupported_version\""));
    let invalid_type = HANDSHAKE.replace("\"version\":1", "\"version\":\"1\"");
    let invalid_type_response = request(&mut stream, invalid_type.as_bytes());
    assert!(invalid_type_response.contains("\"code\":\"protocol.malformed\""));
    let invalid_version = HANDSHAKE.replace("\"0.1.0\"", "\"not-semver\"");
    let invalid_version_response = request(&mut stream, invalid_version.as_bytes());
    assert!(invalid_version_response.contains("\"code\":\"protocol.malformed\""));
    let both_branches = format!(
        "{}\n",
        HANDSHAKE.trim_end().replace(
            "}}",
            "},\"error\":{\"code\":\"protocol.malformed\",\"message\":\"bad\",\"retryable\":false}}",
        )
    );
    let both_response = request(&mut stream, both_branches.as_bytes());
    assert!(both_response.contains("\"code\":\"protocol.malformed\""));
    let unknown_kind = HANDSHAKE.replace("handshake.request", "task.request");
    let unknown_response = request(&mut stream, unknown_kind.as_bytes());
    assert!(unknown_response.contains("\"code\":\"protocol.malformed\""));
    let valid = request(&mut stream, HANDSHAKE.as_bytes());
    assert!(valid.contains("\"kind\":\"handshake.response\""));
    let _ = stop(daemon);
}

#[test]
fn health_is_idempotent_and_only_canonical_empty_payload_is_accepted() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let _ = request(&mut stream, HANDSHAKE.as_bytes());
    let first = request(
        &mut stream,
        health("55555555-5555-4555-8555-555555555555").as_bytes(),
    );
    let second = request(
        &mut stream,
        health("77777777-7777-4777-8777-777777777777").as_bytes(),
    );
    assert!(first.contains("\"status\":\"ok\""));
    assert!(second.contains("\"status\":\"ok\""));
    assert!(first.contains("\"correlation_id\":\"55555555-5555-4555-8555-555555555555\""));
    assert!(second.contains("\"correlation_id\":\"77777777-7777-4777-8777-777777777777\""));
    let nonempty = health("88888888-8888-4888-8888-888888888888")
        .replace("\"payload\":{}", "\"payload\":{\"extra\":1}");
    let invalid = request(&mut stream, nonempty.as_bytes());
    assert!(invalid.contains("\"code\":\"protocol.malformed\""));
    let _ = stop(daemon);
}

#[test]
fn jsonl_framing_is_utf8_newline_delimited_and_inclusive_at_one_mib() {
    let root = root();
    let daemon = start(&root);
    let mut exact = vec![b' '; MAX_LINE - 1];
    exact.push(b'\n');
    let output = jsonl(&root, &exact);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"code\":\"protocol.malformed\""));
    assert!(output.stderr.is_empty());
    let mut oversized = vec![b' '; MAX_LINE];
    oversized.push(b'\n');
    let output = jsonl(&root, &oversized);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"code\":\"protocol.oversized\""));
    let output = jsonl(&root, b"\xff\n");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"code\":\"protocol.malformed\""));
    let _ = stop(daemon);
}

#[test]
fn jsonl_request_errors_stay_on_stdout_and_stream_continues() {
    let root = root();
    let daemon = start(&root);
    let input = [
        b"not json\n".as_slice(),
        HANDSHAKE.as_bytes(),
        health("55555555-5555-4555-8555-555555555555").as_bytes(),
    ]
    .concat();
    let output = jsonl(&root, &input);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert_eq!(stdout.lines().count(), 3);
    assert!(stdout.contains("\"kind\":\"error.response\""));
    assert!(stdout.contains("\"kind\":\"handshake.response\""));
    assert!(stdout.contains("\"kind\":\"health.response\""));
    assert!(output.stderr.is_empty());
    let daemon_output = stop(daemon);
    assert!(daemon_output.stdout.is_empty());
}

#[test]
fn authenticated_socket_errors_stay_on_the_socket_and_daemon_stdout_stays_empty() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let _ = request(&mut stream, HANDSHAKE.as_bytes());
    let unsupported = HANDSHAKE.replace("handshake.request", "unsupported.request");
    let response = request(&mut stream, unsupported.as_bytes());
    assert!(response.contains("\"kind\":\"error.response\""));
    assert!(response.contains("\"code\":\"protocol.malformed\""));
    assert!(response.contains("\"correlation_id\":\"11111111-1111-4111-8111-111111111111\""));
    let output = stop(daemon);
    assert!(output.stdout.is_empty());
}

#[test]
fn present_optional_correlation_id_must_be_a_uuid_string() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let invalid = HANDSHAKE.replace("\"payload\":", "\"correlation_id\":123,\"payload\":");
    let response = request(&mut stream, invalid.as_bytes());
    assert!(response.contains("\"code\":\"protocol.malformed\""));
    let _ = stop(daemon);
}

fn request_id(line: &str) -> String {
    let marker = "\"id\":\"";
    let start = line.find(marker).expect("request id") + marker.len();
    line[start..]
        .split('"')
        .next()
        .expect("request id value")
        .to_string()
}

fn fake_tui_error_server(
    root: &Path,
    code: &str,
    retryable: bool,
    extra: bool,
) -> std::thread::JoinHandle<()> {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("akashic")).expect("socket directory");
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).expect("runtime mode");
    fs::set_permissions(runtime.join("akashic"), fs::Permissions::from_mode(0o700))
        .expect("runtime child mode");
    let listener = std::os::unix::net::UnixListener::bind(socket(root)).expect("fake socket");
    let code = code.to_string();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("client");
        let mut line = String::new();
        std::io::BufReader::new(stream.try_clone().expect("reader"))
            .read_line(&mut line)
            .expect("request");
        let request_id = request_id(&line);
        let extra_field = if extra { ",\"extra\":1" } else { "" };
        writeln!(
            stream,
            "{{\"protocol\":\"akashic.local\",\"version\":1,\"kind\":\"error.response\",\"id\":\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\",\"correlation_id\":\"{request_id}\",\"error\":{{\"code\":\"{code}\",\"message\":\"startup failure\",\"retryable\":{retryable}{extra_field}}}}}"
        )
        .expect("error response");
    })
}

#[test]
fn tui_error_responses_require_exact_schema_and_retryability_mapping() {
    let cases = [
        ("config.invalid", true, false, 3),
        ("protocol.malformed", true, false, 3),
        ("lifecycle.daemon_running", false, false, 3),
        ("internal.unexpected", false, false, 3),
        ("lifecycle.daemon_running", true, false, 4),
        ("authorization.peer_uid", false, true, 3),
    ];
    for (code, retryable, extra, expected_exit) in cases {
        let root = root();
        let server = fake_tui_error_server(&root, code, retryable, extra);
        let output = Command::new(env!("CARGO_BIN_EXE_akashic"))
            .arg("tui")
            .current_dir(&root)
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("HOME", root.join("home"))
            .output()
            .expect("tui");
        server.join().expect("fake daemon");
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{code}/{retryable}/{extra}"
        );
        if !retryable
            || extra
            || code.starts_with("config.")
            || code.starts_with("protocol.")
            || code.starts_with("authorization.")
            || code.starts_with("internal.")
        {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("protocol.malformed"),
                "{code}/{retryable}/{extra}"
            );
        }
    }
}

#[test]
fn strict_json_rejects_non_json_whitespace_and_trailing_commas() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let unicode_space = HANDSHAKE.replacen(":1,", ":1,\u{00a0}", 1);
    let response = request(&mut stream, unicode_space.as_bytes());
    assert!(response.contains("\"code\":\"protocol.malformed\""));
    let trailing = HANDSHAKE.replace("}}\n", "},}\n");
    let response = request(&mut stream, trailing.as_bytes());
    assert!(response.contains("\"code\":\"protocol.malformed\""));
    let _ = stop(daemon);
}

#[test]
fn semver_and_optional_correlation_id_are_validated() {
    let root = root();
    let daemon = start(&root);
    let mut stream = UnixStream::connect(socket(&root)).expect("socket");
    let prerelease = HANDSHAKE.replace("\"0.1.0\"", "\"1.2.3-alpha.1+build.7\"");
    assert!(request(&mut stream, prerelease.as_bytes()).contains("handshake.response"));
    let invalid = HANDSHAKE.replace("\"0.1.0\"", "\"01.2.3\"");
    assert!(request(&mut stream, invalid.as_bytes()).contains("protocol.malformed"));
    let invalid_correlation = HANDSHAKE.replace(
        "\"payload\":",
        "\"correlation_id\":\"not-a-uuid\",\"payload\":",
    );
    assert!(request(&mut stream, invalid_correlation.as_bytes()).contains("protocol.malformed"));
    let valid_correlation = health("88888888-8888-4888-8888-888888888888").replace(
        "\"payload\":",
        "\"correlation_id\":\"33333333-3333-4333-8333-333333333333\",\"payload\":",
    );
    assert!(request(&mut stream, valid_correlation.as_bytes()).contains("health.response"));
    let _ = stop(daemon);
}

#[test]
fn malformed_framed_json_preserves_a_valid_top_level_request_id() {
    let root = root();
    let daemon = start(&root);
    let malformed = HANDSHAKE.replace("}}\n", "}\n");
    let mut client = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .args(["run", "--jsonl"])
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("jsonl");
    client
        .stdin
        .take()
        .expect("stdin")
        .write_all(malformed.as_bytes())
        .expect("malformed input");
    let output = client.wait_with_output().expect("jsonl output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("protocol.malformed"));
    assert!(stdout.contains("\"correlation_id\":\"11111111-1111-4111-8111-111111111111\""));
    let _ = stop(daemon);
}

#[test]
fn partial_jsonl_bytes_survive_a_read_timeout() {
    let root = root();
    let daemon = start(&root);
    let mut client = Command::new(env!("CARGO_BIN_EXE_akashic"))
        .args(["run", "--jsonl"])
        .current_dir(&root)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("jsonl");
    let mut input = client.stdin.take().expect("stdin");
    let split = HANDSHAKE.len() / 2;
    input
        .write_all(&HANDSHAKE.as_bytes()[..split])
        .expect("partial input");
    thread::sleep(Duration::from_millis(100));
    input
        .write_all(&HANDSHAKE.as_bytes()[split..])
        .expect("remainder");
    drop(input);
    let output = client.wait_with_output().expect("jsonl output");
    assert!(String::from_utf8_lossy(&output.stdout).contains("handshake.response"));
    let _ = stop(daemon);
}
