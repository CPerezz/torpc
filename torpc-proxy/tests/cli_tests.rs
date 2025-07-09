use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Local HTTP-to-SOCKS5 proxy"));
}

#[test]
fn test_cli_version() {
    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("torpc-proxy"));
}

#[test]
fn test_config_subcommand() {
    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("# ToRPC Proxy Configuration"))
        .stdout(predicate::str::contains("port = 8545"))
        .stdout(predicate::str::contains("tor_proxy_host = \"127.0.0.1\""))
        .stdout(predicate::str::contains("tor_proxy_port = 9050"));
}

#[test]
fn test_test_subcommand() {
    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    // The test command can either succeed (if Tor is running) or fail (if not)
    let output = cmd.arg("test").output().unwrap();

    // Check that it ran and produced expected output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should always show "Testing Tor connectivity..."
    assert!(
        stdout.contains("Testing Tor connectivity") || stderr.contains("Testing Tor connectivity"),
        "Expected 'Testing Tor connectivity' in output"
    );

    // Should show either success or failure
    let has_success = stdout.contains("Successfully connected to Tor")
        || stderr.contains("Successfully connected to Tor");
    let has_failure = stdout.contains("Failed to connect through Tor")
        || stderr.contains("Failed to connect through Tor");

    assert!(
        has_success || has_failure,
        "Expected either success or failure message"
    );
}

#[test]
fn test_start_with_custom_config() {
    let config_content = r#"
port = 8548
tor_proxy_host = "127.0.0.1"
tor_proxy_port = 9051
onion_endpoint = "custom.onion:8545"
log_level = "debug"
"#;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("--config")
        .arg(temp_file.path())
        .arg("start")
        .timeout(std::time::Duration::from_millis(500));

    // The command will timeout (which is expected) but we can check
    // that it tried to start with the right config
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // Should show it's using the custom config values
    assert!(
        combined.contains("8548")
            || combined.contains("custom.onion")
            || combined.contains("debug"),
        "Expected custom config values in output. Got: {combined}"
    );
}

#[test]
fn test_start_with_port_override() {
    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("start")
        .arg("--port")
        .arg("8549")
        .timeout(std::time::Duration::from_millis(500));

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // Should show it's using port 8549
    assert!(
        combined.contains("8549"),
        "Expected port 8549 in output. Got: {combined}"
    );
}

#[test]
fn test_start_with_onion_override() {
    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("start")
        .arg("--port")
        .arg("0") // Use port 0 to get auto-assigned port
        .arg("--onion")
        .arg("mytest.onion:8545")
        .timeout(std::time::Duration::from_millis(500));

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // Should show it's forwarding to mytest.onion
    assert!(
        combined.contains("mytest.onion"),
        "Expected mytest.onion in output. Got: {combined}"
    );
}

#[test]
fn test_start_without_onion_endpoint() {
    // Create a config without onion_endpoint
    let config_content = r#"
port = 8550
tor_proxy_host = "127.0.0.1"
tor_proxy_port = 9050
log_level = "info"
"#;

    let temp_file = NamedTempFile::new().unwrap();
    fs::write(temp_file.path(), config_content).unwrap();

    let mut cmd = Command::cargo_bin("torpc-proxy").unwrap();
    cmd.arg("--config")
        .arg(temp_file.path())
        .arg("start")
        .assert()
        .code(1) // Should exit with error code
        .stdout(predicate::str::contains("No onion endpoint specified"));
}
