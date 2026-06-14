//! Dogfood demo for the `deploy_webhooks` feature.
//!
//! Spawns a local HTTP listener, sets up a temporary `ready-set-encrypt`
//! manifest with a `self-issued` secret that fires a deploy webhook
//! against that listener, then invokes the plugin's CLI exactly the way
//! `ready-set rotate --confirm` would. Prints what the listener
//! received so you can eyeball the substituted `{{value}}` payload.
//!
//! Run it from the workspace root:
//!
//! ```sh
//! cargo run --example deploy_webhook_demo -p ready-set-encrypt
//! ```
//!
//! Nothing in your actual `.ready-set/` directory or env is modified.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[allow(clippy::too_many_lines)]
fn main() {
    // 1) Spawn a tiny one-shot HTTP server that records what it
    // receives. Same robustness pattern as the e2e tests so ureq
    // doesn't race the response read.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind localhost");
    let port = listener.local_addr().unwrap().port();
    println!("[demo] listener on http://127.0.0.1:{port}/notify");

    let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let captured_inner = Arc::clone(&captured);
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 4096];
        let mut headers_done: Option<usize> = None;
        let mut content_length: usize = 0;
        loop {
            let n = match stream.read(&mut tmp) {
                Ok(n) if n > 0 => n,
                _ => break,
            };
            buf.extend_from_slice(&tmp[..n]);
            if headers_done.is_none()
                && let Some(end) = find_subslice(&buf, b"\r\n\r\n")
            {
                headers_done = Some(end + 4);
                content_length = parse_content_length(&buf[..end]);
            }
            if let Some(hd) = headers_done
                && buf.len() >= hd + content_length
            {
                break;
            }
        }
        *captured_inner.lock().unwrap() = Some(buf);
        let body = b"{\"ok\":true}";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        drop(stream.write_all(response.as_bytes()));
        drop(stream.write_all(body));
        drop(stream.flush());
        drop(stream.shutdown(Shutdown::Write));
        let mut sink = [0u8; 256];
        while stream.read(&mut sink).unwrap_or(0) > 0 {}
    });
    std::thread::sleep(Duration::from_millis(30));

    // 2) Stand up a throwaway project: source file referencing a fake
    // env var so the inventory picks it up, plus a manifest that wires
    // up the `self-issued` + `deploy_webhooks` configuration.
    let workspace = tempfile::tempdir().expect("tempdir");
    let project = workspace.path();
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("src/main.rs"),
        "fn main(){let _=std::env::var(\"DEMO_TOKEN\").unwrap();}\n",
    )
    .unwrap();
    let manifest_dir = project.join(".ready-set/plugins/secrets");
    std::fs::create_dir_all(&manifest_dir).unwrap();
    let manifest = format!(
        r#"[ready-set-encrypt]
schema_version = 1
default_cadence_days = 90

[secret.DEMO_TOKEN]
backend = "self-issued"
target_path = "secrets/demo-token"

[[secret.DEMO_TOKEN.deploy_webhooks]]
url = "http://127.0.0.1:{port}/notify"
body = '{{"event":"rotated","secret":"DEMO_TOKEN","value":"{{{{value}}}}"}}'
"#
    );
    std::fs::write(manifest_dir.join("manifest.toml"), manifest).unwrap();
    println!(
        "[demo] wrote manifest at {}/manifest.toml",
        manifest_dir.display()
    );

    // 3) Run the plugin's `__go rotation --confirm` exactly the way
    // the ready-set dispatcher does. Use the binary cargo just built.
    let plugin_path = current_plugin_binary();
    println!(
        "[demo] invoking {} __go rotation --confirm",
        plugin_path.display()
    );
    let out = Command::new(&plugin_path)
        .args(["__go", "rotation", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(project)
        .output()
        .expect("spawn plugin");
    println!("[demo] plugin exit: {}", out.status);
    println!(
        "[demo] plugin stdout: {}",
        String::from_utf8_lossy(&out.stdout).trim()
    );
    if !out.stderr.is_empty() {
        println!(
            "[demo] plugin stderr: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    // 4) Inspect what the webhook listener saw.
    let received = captured
        .lock()
        .unwrap()
        .clone()
        .expect("webhook should have been called");
    println!();
    println!("--- webhook received {} bytes ---", received.len());
    println!("{}", String::from_utf8_lossy(&received));
    println!("--- end webhook payload ---");

    let target = project.join("secrets/demo-token");
    if let Ok(value) = std::fs::read_to_string(&target) {
        println!();
        println!(
            "[demo] target_path contents ({} chars): {}",
            value.trim().len(),
            value.trim()
        );
    }

    let audit = project.join(".ready-set/plugins/secrets/rotations.jsonl");
    if let Ok(log) = std::fs::read_to_string(&audit) {
        println!();
        println!("[demo] audit log entry:");
        println!("{}", log.trim());
    }

    if !out.status.success() {
        std::process::exit(1);
    }
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_content_length(headers: &[u8]) -> usize {
    let text = std::str::from_utf8(headers).unwrap_or("");
    for line in text.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            return v.trim().parse().unwrap_or(0);
        }
    }
    0
}

/// Locate the freshly-built plugin binary so the example runs against
/// the in-tree code. `cargo run --example` puts us in
/// `target/debug/examples/`; the sibling binary is one directory up.
fn current_plugin_binary() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let parent = exe.parent().expect("exe parent");
    let target_dir = parent.parent().expect("examples parent");
    let candidate = target_dir.join(if cfg!(windows) {
        "ready-set-encrypt.exe"
    } else {
        "ready-set-encrypt"
    });
    if !candidate.exists() {
        eprintln!(
            "[demo] expected plugin binary at {}; build it first with `cargo build -p ready-set-encrypt`",
            candidate.display()
        );
        std::process::exit(2);
    }
    candidate
}
