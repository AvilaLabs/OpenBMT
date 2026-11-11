//! BX-44 to BX-48: the Box's way to a vendor over the system's curl, with
//! no network. A fake curl is a shell script in a directory of the test's
//! own: it records what it was given and prints a reply and a status.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::https::{
    error_text, exit_refusal, invocation, read_capped, split_status, CurlRunner, HttpsKind,
    HttpsRefusal, HttpsReply, HttpsRequest, HttpsRunner, Method, Transport, MAX_BODY_BYTES,
    MAX_REPLY_BYTES, RATE_LIMIT_WAIT_SECS,
};
use crate::say::note_words;
use openbmt_pos::PosError;

const SECRET: &str = "sq0atp-SECRET-token-0123456789";

/// A scratch directory that is removed afterwards.
pub(super) struct Dir(pub PathBuf);

impl Dir {
    pub(super) fn new(name: &str) -> Self {
        let base = std::env::var_os("OPENBMT_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("openbmt-box-https-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fake curl: a script that writes its arguments, its standard input and
/// its environment into `dir`, then prints `body` and `status` as curl's
/// `--write-out` would, and exits with `exit`.
pub(super) fn fake_curl(dir: &Dir, body: &str, status: &str, exit: i32) -> String {
    let script = format!(
        "#!/bin/sh\n\
         echo x >> '{calls}'\n\
         for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{args}'; done\n\
         /bin/cat > '{stdin}'\n\
         /usr/bin/env > '{env}'\n\
         printf '%s%s' '{body}' '{status}'\n\
         exit {exit}\n",
        calls = dir.path("calls").display(),
        args = dir.path("args").display(),
        stdin = dir.path("stdin").display(),
        env = dir.path("env").display(),
    );
    let path = dir.path("curl");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

/// Run a request, again if the system says the script is busy (another
/// thread of the test binary had it open for writing when this one forked).
pub(super) fn run_retry(
    runner: &mut CurlRunner,
    request: &HttpsRequest,
) -> Result<HttpsReply, HttpsRefusal> {
    for _ in 0..50 {
        match runner.run(request) {
            Err(HttpsRefusal::CurlNotRun {
                kind: std::io::ErrorKind::ExecutableFileBusy,
                ..
            }) => std::thread::sleep(std::time::Duration::from_millis(20)),
            other => return other,
        }
    }
    runner.run(request)
}

fn read(dir: &Dir, name: &str) -> String {
    fs::read_to_string(dir.path(name)).unwrap_or_default()
}

fn request() -> HttpsRequest {
    let mut r = HttpsRequest::new(
        Method::Post,
        "https://connect.squareup.com/v2/orders/search",
    );
    r.headers = vec![
        ("Authorization".into(), format!("Bearer {SECRET}")),
        ("Accept".into(), "application/json".into()),
    ];
    r.body = Some("{\"limit\":100}".into());
    r
}

// ---- BX-44: curl only ----

#[test]
fn bx_44_a_missing_curl_is_a_refusal_with_words() {
    let mut runner = CurlRunner::new("/nonexistent-dir/curl");
    let refused = runner.run(&request()).unwrap_err();
    assert_eq!(
        refused,
        HttpsRefusal::CurlMissing {
            path: "/nonexistent-dir/curl".into()
        }
    );
    assert!(refused.to_string().starts_with("BX-44: "));
    assert!(refused
        .to_string()
        .contains("curl is not installed on this Box"));
    let words = note_words(&refused.note());
    assert_eq!(words.heading, "curl is not installed");
    assert!(words.what.contains("curl is not installed on this Box"));
    assert_eq!(words.code, Some("BX-44"));
}

#[test]
fn bx_44_a_curl_the_system_will_not_run_is_not_a_panic() {
    let dir = Dir::new("bx44-norun");
    let path = dir.path("curl");
    fs::write(&path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let mut runner = CurlRunner::new(path.display().to_string());
    let refused = runner.run(&request()).unwrap_err();
    assert_eq!(refused.kind(), HttpsKind::CurlNotRun);
    assert!(refused.to_string().starts_with("BX-44: "));
    assert_eq!(refused.pos_error(), PosError::Unavailable);
}

#[test]
fn bx_44_the_real_runner_runs_the_curl_it_was_given_as_a_child_process() {
    let dir = Dir::new("bx44-child");
    let curl = fake_curl(&dir, "{\"orders\":[]}", "200", 0);
    let mut runner = CurlRunner::new(curl);
    let reply = run_retry(&mut runner, &request()).unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, b"{\"orders\":[]}");
    assert_eq!(read(&dir, "calls").lines().count(), 1);
}

#[test]
fn bx_44_the_box_depends_on_no_tls_or_http_crate() {
    let manifest = include_str!("../../Cargo.toml");
    let deps: Vec<&str> = manifest
        .lines()
        .skip_while(|l| l.trim() != "[dependencies]")
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert!(deps.len() >= 6, "{deps:?}");
    for dep in deps {
        assert!(dep.starts_with("openbmt-"), "{dep}");
        assert!(dep.contains("path = "), "{dep}");
    }
}

// ---- BX-45: the token is not in the arguments or the environment ----

#[test]
fn bx_45_the_token_is_in_the_config_and_not_in_the_arguments() {
    let made = invocation(&request()).unwrap();
    assert!(made.config.contains(&format!("Bearer {SECRET}")));
    assert!(made.config.starts_with("header = \"Authorization: Bearer "));
    assert!(made
        .config
        .contains("data-binary = \"{\\\"limit\\\":100}\""));
    for arg in &made.args {
        assert!(!arg.contains(SECRET), "{arg}");
        assert!(!arg.contains("Bearer"), "{arg}");
        assert!(!arg.contains("limit"), "{arg}");
    }
    assert_eq!(made.args.first().map(String::as_str), Some("-q"));
    assert_eq!(
        made.args.last().map(String::as_str),
        Some("https://connect.squareup.com/v2/orders/search")
    );
    // Nor is it in what the request prints about itself.
    assert!(!format!("{:?}", request()).contains(SECRET));
    assert!(!format!("{made:?}").contains(SECRET));
}

#[test]
fn bx_45_curl_is_given_the_config_on_standard_input_and_an_empty_environment() {
    let dir = Dir::new("bx45-real");
    let curl = fake_curl(&dir, "{}", "200", 0);
    std::env::set_var("OPENBMT_BOX_TEST_PROXY_PROBE", "leaks");
    std::env::set_var("https_proxy", "http://proxy.invalid:3128");
    let mut runner = CurlRunner::new(curl);
    run_retry(&mut runner, &request()).unwrap();
    let args = read(&dir, "args");
    let stdin = read(&dir, "stdin");
    let env = read(&dir, "env");
    assert!(!args.contains(SECRET), "{args}");
    assert!(!args.contains("Authorization"), "{args}");
    assert!(stdin.contains(&format!("header = \"Authorization: Bearer {SECRET}\"")));
    assert!(stdin.contains("data-binary = "));
    assert!(!env.contains(SECRET), "{env}");
    assert!(!env.contains("OPENBMT_BOX_TEST_PROXY_PROBE"), "{env}");
    assert!(!env.contains("https_proxy"), "{env}");
    assert_eq!(args.lines().next(), Some("-q"));
}

#[test]
fn bx_45_a_header_or_body_that_cannot_be_written_into_a_config_is_refused_first() {
    let dir = Dir::new("bx45-bad");
    let curl = fake_curl(&dir, "{}", "200", 0);
    let mut runner = CurlRunner::new(curl);
    let mut with_newline = request();
    with_newline.headers[0].1 = "Bearer a\nheader = \"Evil: 1\"".into();
    let mut at_sign = request();
    at_sign.body = Some("@/etc/passwd".into());
    let mut bad_name = request();
    bad_name.headers[1].0 = "Accept: x\n".into();
    let mut control = request();
    control.body = Some("a\u{1}b".into());
    for r in [with_newline, at_sign, bad_name, control] {
        let refused = runner.run(&r).unwrap_err();
        assert_eq!(refused.kind(), HttpsKind::BadRequest, "{r:?}");
        assert!(refused.to_string().starts_with("BX-45: "));
    }
    // None of them ran curl.
    assert_eq!(read(&dir, "calls"), "");
}

#[test]
fn bx_45_quotes_and_backslashes_are_escaped_for_curl_config() {
    let mut r = request();
    r.body = Some("{\"a\":\"b\\\\c\",\"t\":\"x\ty\"}".into());
    let made = invocation(&r).unwrap();
    assert!(
        made.config
            .contains("data-binary = \"{\\\"a\\\":\\\"b\\\\\\\\c\\\",\\\"t\\\":\\\"x\\ty\\\"}\"\n"),
        "{}",
        made.config
    );
    assert_eq!(made.config.lines().count(), 3);
}

// ---- BX-46: the flags and the caps ----

#[test]
fn bx_46_the_flags_are_https_only_tls_12_and_the_limits() {
    let made = invocation(&request()).unwrap();
    let args = made.args.join(" ");
    for flag in [
        "--config - ",
        "--silent",
        "--show-error",
        "--proto =https",
        "--tlsv1.2",
        "--max-time 30",
        "--connect-timeout 15",
        "--max-filesize 8388608",
        "--request POST",
        "--write-out %{http_code}",
    ] {
        assert!(args.contains(flag), "{flag} is not in {args}");
    }
    assert!(!args.contains("--socks5-hostname"));
    assert!(!args.contains(" -L") && !args.contains("--location"));
    let get = HttpsRequest::new(Method::Get, "https://api.clover.com/v3/merchants/M1/orders");
    assert!(invocation(&get)
        .unwrap()
        .args
        .join(" ")
        .contains("--request GET"));
    for url in [
        "http://connect.squareup.com/",
        "ftp://x/",
        "https://x/ y",
        "https://x/\n",
        "file:///etc/passwd",
    ] {
        let r = HttpsRequest::new(Method::Get, url);
        assert_eq!(
            invocation(&r).unwrap_err().kind(),
            HttpsKind::BadRequest,
            "{url}"
        );
    }
}

#[test]
fn bx_46_a_reply_over_the_cap_is_refused_and_curl_is_ended() {
    let dir = Dir::new("bx46-cap");
    let body = "x".repeat(200);
    let curl = fake_curl(&dir, &body, "200", 0);
    let mut runner = CurlRunner::new(curl);
    let mut r = request();
    r.max_reply = 64;
    let refused = run_retry(&mut runner, &r).unwrap_err();
    assert_eq!(refused, HttpsRefusal::ReplyTooLarge { cap: 64 });
    assert!(refused.to_string().starts_with("BX-46: "));
    // A reply at the cap is taken.
    let curl = fake_curl(&dir, &"y".repeat(64), "200", 0);
    let mut runner = CurlRunner::new(curl);
    let reply = run_retry(&mut runner, &r).unwrap();
    assert_eq!(reply.body.len(), 64);
    // The cap is the bound of PS-90 and the body bound is small.
    assert_eq!(MAX_REPLY_BYTES, 8 * 1024 * 1024);
    let mut big = request();
    big.body = Some("x".repeat(MAX_BODY_BYTES + 1));
    assert_eq!(invocation(&big).unwrap_err().kind(), HttpsKind::BadRequest);
    // And curl's own refusal for a file over the cap is the same refusal.
    assert_eq!(
        exit_refusal(Some(63), None, String::new()).kind(),
        HttpsKind::ReplyTooLarge
    );
}

#[test]
fn bx_46_curl_exit_codes_are_refusals_with_words() {
    let kind = |code: i32| exit_refusal(Some(code), None, "text".into()).kind();
    assert_eq!(kind(28), HttpsKind::Timeout);
    assert_eq!(kind(6), HttpsKind::Unreachable);
    assert_eq!(kind(7), HttpsKind::Unreachable);
    assert_eq!(kind(56), HttpsKind::Unreachable);
    for tls in [35, 51, 58, 60, 77, 83] {
        assert_eq!(kind(tls), HttpsKind::Tls, "{tls}");
    }
    assert_eq!(kind(2), HttpsKind::CurlFailed);
    assert_eq!(
        exit_refusal(None, None, "killed".into()),
        HttpsRefusal::CurlFailed {
            code: None,
            text: "killed".into()
        }
    );
    let dir = Dir::new("bx46-exit");
    let curl = fake_curl(&dir, "", "", 28);
    let mut runner = CurlRunner::new(curl);
    assert_eq!(
        run_retry(&mut runner, &request()),
        Err(HttpsRefusal::Timeout)
    );
    for kind in HttpsKind::ALL {
        let words = note_words(&format!(
            "the POS was not reached ({}): {}: detail",
            kind.tag(),
            kind.code()
        ));
        assert_eq!(words.code, Some(kind.code()));
        assert_eq!(HttpsKind::from_tag(kind.tag()), Some(kind));
    }
}

#[test]
fn bx_46_an_output_with_no_status_is_a_refusal() {
    assert_eq!(split_status(b"{}200").unwrap(), (200, &b"{}"[..]));
    assert_eq!(split_status(b"404").unwrap(), (404, &b""[..]));
    for bad in [&b""[..], b"20", b"{}2x0", b"{}000", b"{}999"] {
        assert_eq!(split_status(bad), Err(HttpsRefusal::BadOutput), "{bad:?}");
    }
    let dir = Dir::new("bx46-noout");
    let curl = fake_curl(&dir, "no status here", "", 0);
    let mut runner = CurlRunner::new(curl);
    assert_eq!(
        run_retry(&mut runner, &request()),
        Err(HttpsRefusal::BadOutput)
    );
    // The reading functions are bounded.
    let mut data: &[u8] = b"abcdef";
    assert_eq!(read_capped(&mut data, 6).unwrap(), Some(b"abcdef".to_vec()));
    let mut data: &[u8] = b"abcdefg";
    assert_eq!(read_capped(&mut data, 6).unwrap(), None);
    assert_eq!(error_text(b"curl: (7) a\x01b\nsecond"), "curl: (7) a b");
    assert!(error_text(&[b'z'; 5_000]).len() <= 200);
}

// ---- BX-47: Tor and no fallback ----

#[test]
fn bx_47_with_a_proxy_curl_resolves_names_through_it() {
    let mut r = request();
    assert!(!invocation(&r)
        .unwrap()
        .args
        .contains(&"--socks5-hostname".to_string()));
    r.socks5 = Some("127.0.0.1:9050".into());
    let args = invocation(&r).unwrap().args;
    let at = args.iter().position(|a| a == "--socks5-hostname").unwrap();
    assert_eq!(args.get(at + 1).map(String::as_str), Some("127.0.0.1:9050"));
    // Not --socks5, which would resolve the name on this machine.
    assert!(!args.contains(&"--socks5".to_string()));
    for bad in [
        "nocolon",
        "host:0",
        "host:99999",
        ":9050",
        "a b:9050",
        "h:p",
    ] {
        r.socks5 = Some(bad.into());
        assert_eq!(
            invocation(&r).unwrap_err().kind(),
            HttpsKind::BadRequest,
            "{bad}"
        );
    }
}

#[test]
fn bx_47_a_box_that_does_not_know_goes_through_tor() {
    let t = Transport::new();
    assert!(t.tor());
    t.set_tor(false);
    assert!(!t.tor());
    t.set_tor(true);
    assert!(t.tor());
}

#[test]
fn bx_47_a_proxy_that_cannot_be_used_is_refused_and_nothing_else_is_tried() {
    let dir = Dir::new("bx47-proxy");
    let mut r = request();
    r.socks5 = Some("127.0.0.1:9".into());
    for code in [5, 7, 97] {
        let _ = fs::remove_file(dir.path("calls"));
        let curl = fake_curl(&dir, "", "", code);
        let mut runner = CurlRunner::new(curl);
        let refused = run_retry(&mut runner, &r).unwrap_err();
        assert_eq!(
            refused,
            HttpsRefusal::ProxyUnreachable {
                proxy: "127.0.0.1:9".into()
            },
            "exit {code}"
        );
        assert!(refused.to_string().starts_with("BX-47: "));
        // One run of curl, with the proxy in it: no direct try after it.
        assert_eq!(read(&dir, "calls").lines().count(), 1);
        assert!(read(&dir, "args").contains("--socks5-hostname"));
    }
    // Without a proxy the same exit is only "not reached".
    let curl = fake_curl(&dir, "", "", 7);
    let mut runner = CurlRunner::new(curl);
    assert_eq!(
        run_retry(&mut runner, &request()).unwrap_err().kind(),
        HttpsKind::Unreachable
    );
    let words = note_words(
        &HttpsRefusal::ProxyUnreachable {
            proxy: "127.0.0.1:9".into(),
        }
        .note(),
    );
    assert_eq!(words.heading, "Tor proxy not usable");
    assert!(words.what.contains("without it"));
}

// ---- BX-48: statuses ----

fn reply(status: u16) -> HttpsReply {
    HttpsReply {
        status,
        body: b"{}".to_vec(),
    }
}

#[test]
fn bx_48_a_status_of_2xx_is_read() {
    for status in [200, 201, 204, 299] {
        assert_eq!(reply(status).into_body(), Ok(b"{}".to_vec()));
    }
}

#[test]
fn bx_48_401_and_403_say_the_token_was_refused_and_what_to_do() {
    for status in [401, 403] {
        let refused = reply(status).into_body().unwrap_err();
        assert_eq!(refused, HttpsRefusal::TokenRefused { status });
        assert_eq!(refused.pos_error(), PosError::SignInRefused);
        assert!(refused.to_string().starts_with("BX-48: "));
        let words = note_words(&refused.note());
        assert_eq!(words.heading, "Till refused the token");
        assert!(words.next.contains("new read-only token"));
        assert!(words.next.contains("pos-token"));
        assert_eq!(words.code, Some("BX-48"));
    }
}

#[test]
fn bx_48_429_backs_off_and_other_statuses_are_refused_with_the_status() {
    let refused = reply(429).into_body().unwrap_err();
    assert_eq!(
        refused,
        HttpsRefusal::RateLimited {
            retry_after_secs: RATE_LIMIT_WAIT_SECS
        }
    );
    assert_eq!(
        refused.pos_error(),
        PosError::RateLimited {
            retry_after_secs: 60
        }
    );
    assert_eq!(
        reply(302).into_body().unwrap_err(),
        HttpsRefusal::Redirected { status: 302 }
    );
    let e = reply(404).into_body().unwrap_err();
    assert_eq!(e, HttpsRefusal::Status { status: 404 });
    assert!(e.to_string().contains("404"));
    assert!(matches!(e.pos_error(), PosError::BadResponse(_)));
    assert_eq!(
        reply(503).into_body().unwrap_err().pos_error(),
        PosError::Unavailable
    );
    assert_eq!(
        reply(100).into_body().unwrap_err(),
        HttpsRefusal::Status { status: 100 }
    );
}

#[test]
fn bx_48_the_status_curl_wrote_reaches_the_caller() {
    let dir = Dir::new("bx48-status");
    for status in ["401", "429", "500"] {
        let curl = fake_curl(&dir, "{\"errors\":[]}", status, 0);
        let mut runner = CurlRunner::new(curl);
        let reply = run_retry(&mut runner, &request()).unwrap();
        assert_eq!(reply.status.to_string(), status);
        assert!(reply.into_body().is_err());
    }
}
