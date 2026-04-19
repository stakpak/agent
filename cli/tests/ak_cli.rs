use std::process::Command;

#[test]
fn ak_status_bootstraps_default_config_on_clean_home() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let home = temp_dir.path();

    let output = Command::new(env!("CARGO_BIN_EXE_stakpak"))
        .arg("ak")
        .arg("status")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("STAKPAK_PROFILE")
        .output()
        .expect("run stakpak ak status");

    assert!(
        output.status.success(),
        "ak status failed: stdout={} stderr= {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Store:"), "stdout was: {stdout}");
    assert!(stdout.contains("Files: 0"), "stdout was: {stdout}");

    assert!(
        home.join(".stakpak/config.toml").is_file(),
        "expected ak status to bootstrap ~/.stakpak/config.toml"
    );
}
