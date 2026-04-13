// Enterprise CLI subcommand tests
// Tests the CLI argument parsing for all enterprise subcommands

use std::process::Command;

fn ryuo_binary() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ryuo"));
    cmd.env("PYO3_USE_ABI3_FORWARD_COMPATIBILITY", "1");
    cmd
}

#[test]
fn test_cli_help() {
    let output = ryuo_binary().arg("--help").output().expect("Failed to run ryuo --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RYUO"), "Help output should mention RYUO");
    assert!(stdout.contains("secret"), "Help should list secret subcommand");
    assert!(stdout.contains("user"), "Help should list user subcommand");
    assert!(stdout.contains("dag"), "Help should list dag subcommand");
    assert!(stdout.contains("pool"), "Help should list pool subcommand");
    assert!(stdout.contains("config"), "Help should list config subcommand");
}

#[test]
fn test_cli_version() {
    let output = ryuo_binary().arg("--version").output().expect("Failed to run ryuo --version");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.7.0"), "Should show version 0.7.0");
}

#[test]
fn test_secret_help() {
    let output = ryuo_binary().args(["secret", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"), "Secret should have list subcommand");
    assert!(stdout.contains("get") || stdout.contains("Get"), "Secret should have get subcommand");
    assert!(stdout.contains("set") || stdout.contains("Set"), "Secret should have set subcommand");
    assert!(stdout.contains("delete") || stdout.contains("Delete"), "Secret should have delete subcommand");
}

#[test]
fn test_user_help() {
    let output = ryuo_binary().args(["user", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"));
    assert!(stdout.contains("create") || stdout.contains("Create"));
}

#[test]
fn test_team_help() {
    let output = ryuo_binary().args(["team", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"));
    assert!(stdout.contains("create") || stdout.contains("Create"));
}

#[test]
fn test_rbac_help() {
    let output = ryuo_binary().args(["rbac", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list-roles") || stdout.contains("ListRoles"));
    assert!(stdout.contains("assign") || stdout.contains("Assign"));
}

#[test]
fn test_token_help() {
    let output = ryuo_binary().args(["token", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"));
    assert!(stdout.contains("create") || stdout.contains("Create"));
    assert!(stdout.contains("revoke") || stdout.contains("Revoke"));
}

#[test]
fn test_dag_help() {
    let output = ryuo_binary().args(["dag", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"));
    assert!(stdout.contains("trigger") || stdout.contains("Trigger"));
    assert!(stdout.contains("pause") || stdout.contains("Pause"));
}

#[test]
fn test_pool_help() {
    let output = ryuo_binary().args(["pool", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"));
    assert!(stdout.contains("create") || stdout.contains("Create"));
}

#[test]
fn test_config_show_requires_db() {
    let output = ryuo_binary().args(["config", "show"]).output().expect("Failed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Config commands require --database-url, so should error without it
    assert!(!output.status.success() || stderr.contains("database") || stderr.contains("DATABASE"));
}

#[test]
fn test_config_export_requires_db() {
    let output = ryuo_binary().args(["config", "export"]).output().expect("Failed");
    // Should fail without database URL
    assert!(!output.status.success());
}

#[test]
fn test_connector_list_requires_db() {
    let output = ryuo_binary().args(["connector", "list"]).output().expect("Failed");
    // Should fail without database URL
    assert!(!output.status.success());
}

#[test]
fn test_swarm_status_requires_db() {
    let output = ryuo_binary().args(["swarm", "status"]).output().expect("Failed");
    // Should fail without database URL  
    assert!(!output.status.success());
}

#[test]
fn test_audit_help() {
    let output = ryuo_binary().args(["audit", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("recent") || stdout.contains("Recent"));
}

#[test]
fn test_compliance_help() {
    let output = ryuo_binary().args(["compliance", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("List"));
    assert!(stdout.contains("status") || stdout.contains("Status"));
}

#[test]
fn test_lineage_help() {
    let output = ryuo_binary().args(["lineage", "--help"]).output().expect("Failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run") || stdout.contains("Run"));
    assert!(stdout.contains("datasets") || stdout.contains("Datasets"));
}
