use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_SANDBOX_ID: AtomicU64 = AtomicU64::new(0);

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    codex_home: PathBuf,
}

impl Sandbox {
    fn new(accounts: Value) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = NEXT_SANDBOX_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "hangar-cli-test-{}-{unique}-{sequence}",
            std::process::id()
        ));
        let home = root.join("home");
        let codex_home = root.join("codex");
        std::fs::create_dir_all(home.join(".hangar")).unwrap();
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::write(
            home.join(".hangar/accounts.json"),
            serde_json::to_vec_pretty(&accounts).unwrap(),
        )
        .unwrap();
        Self {
            root,
            home,
            codex_home,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hangar"))
            .args(args)
            .env("HANGAR_TEST_HOME", &self.home)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CODEX_HOME", &self.codex_home)
            .env("HANGAR_NO_UPDATE", "1")
            .output()
            .unwrap()
    }

    fn accounts_path(&self) -> PathBuf {
        self.home.join(".hangar/accounts.json")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn account(id: &str, email: &str, stale: bool) -> Value {
    json!({
        "id": id,
        "email": email,
        "access_token": "test-access-secret",
        "refresh_token": "test-refresh-secret",
        "id_token": "test-id-secret",
        "expires_at": 123,
        "stale": stale,
        "account_id": null,
        "organization_id": null
    })
}

fn fixture(accounts: Vec<Value>, current: Option<&str>) -> Value {
    json!({
        "accounts": accounts,
        "current_account_id": current,
    })
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn list_json_is_structured_and_redacted() {
    let sandbox = Sandbox::new(fixture(
        vec![account("id-a", "alice@example.com", false)],
        Some("id-a"),
    ));
    let output = sandbox.run(&["list", "--json"]);
    assert!(output.status.success(), "stderr={}", text(&output.stderr));
    let stdout = text(&output.stdout);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["accounts"][0]["email"], "alice@example.com");
    assert_eq!(value["accounts"][0]["current"], true);
    for secret in [
        "test-access-secret",
        "test-refresh-secret",
        "test-id-secret",
    ] {
        assert!(!stdout.contains(secret));
    }
    for field in ["access_token", "refresh_token", "id_token"] {
        assert!(!stdout.contains(field));
    }
}

#[test]
fn unknown_argument_exits_with_usage_code() {
    let sandbox = Sandbox::new(fixture(vec![], None));
    let output = sandbox.run(&["--unknown-option"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!text(&output.stderr).is_empty());
}

#[test]
fn json_argument_error_is_structured() {
    let sandbox = Sandbox::new(fixture(vec![], None));
    let output = sandbox.run(&["--json", "--unknown-option"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["ok"], false);
    assert_eq!(error["code"], 2);
}

#[test]
fn remove_without_yes_is_non_interactive_and_preserves_accounts() {
    let sandbox = Sandbox::new(fixture(
        vec![
            account("id-a", "alice@example.com", false),
            account("id-b", "bob@example.com", false),
        ],
        Some("id-a"),
    ));
    let before = std::fs::read(sandbox.accounts_path()).unwrap();
    let output = sandbox.run(&["remove", "bob@example.com"]);
    assert_eq!(output.status.code(), Some(2));
    let after = std::fs::read(sandbox.accounts_path()).unwrap();
    assert_eq!(before, after);
}

#[test]
fn ambiguous_email_fails_without_touching_official_auth() {
    let sandbox = Sandbox::new(fixture(
        vec![
            account("id-a", "same@example.com", false),
            account("id-b", "same@example.com", false),
        ],
        None,
    ));
    let before = std::fs::read(sandbox.accounts_path()).unwrap();
    let output = sandbox.run(&["switch", "same@example.com", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(!sandbox.codex_home.join("auth.json").exists());
    assert_eq!(before, std::fs::read(sandbox.accounts_path()).unwrap());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["code"], 3);
}

#[test]
fn stale_switch_has_auth_exit_code_and_does_not_write_official_auth() {
    let sandbox = Sandbox::new(fixture(
        vec![account("id-a", "alice@example.com", true)],
        None,
    ));
    let output = sandbox.run(&["switch", "id-a", "--json"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(!sandbox.codex_home.join("auth.json").exists());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["code"], 4);
}

#[test]
fn stale_quota_has_auth_exit_code_without_network_access() {
    let sandbox = Sandbox::new(fixture(
        vec![account("id-a", "alice@example.com", true)],
        None,
    ));
    let output = sandbox.run(&["quota", "id-a", "--json"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["code"], 4);
}

#[test]
fn confirmed_remove_deletes_only_the_selected_inactive_account() {
    let sandbox = Sandbox::new(fixture(
        vec![
            account("id-a", "alice@example.com", false),
            account("id-b", "bob@example.com", false),
        ],
        Some("id-a"),
    ));
    let output = sandbox.run(&["remove", "id-b", "--yes", "--json"]);
    assert!(output.status.success(), "stderr={}", text(&output.stderr));
    let saved: Value =
        serde_json::from_slice(&std::fs::read(sandbox.accounts_path()).unwrap()).unwrap();
    assert_eq!(saved["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(saved["accounts"][0]["id"], "id-a");
    assert_eq!(saved["current_account_id"], "id-a");
}

#[test]
fn current_reports_the_selected_account() {
    let sandbox = Sandbox::new(fixture(
        vec![account("id-a", "alice@example.com", false)],
        Some("id-a"),
    ));
    let output = sandbox.run(&["current"]);
    assert!(output.status.success());
    assert_eq!(text(&output.stdout).trim(), "id-a\talice@example.com");
}
