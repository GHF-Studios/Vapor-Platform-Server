use std::process::Command;

fn helper(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vapor_platform_ops"))
        .args(args)
        .output()
        .expect("run the Platform operations helper")
}

#[test]
fn ssh_help_is_available_on_every_build_host() {
    let output = helper(&["ssh", "--help"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ssh show <request-id>"));
    assert!(stdout.contains("ssh verify <request-id>"));
    assert!(stdout.contains("ssh grant <request-id>"));
}

#[test]
fn ssh_grant_rejects_unknown_capability_before_platform_guard() {
    let output = helper(&[
        "ssh",
        "grant",
        "fb9ef006-f6ad-420c-9ad7-31337b2b1e52",
        "--capability",
        "made_up_admin",
        "--duration",
        "8h",
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported capability"));
}

#[test]
fn ssh_operations_are_explicitly_unavailable_off_the_linux_vps() {
    if cfg!(unix) {
        return;
    }

    let output = helper(&["ssh", "list"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("require a Linux VPS"),
        "stderr was: {stderr:?}"
    );
}
