use std::process::Command;

const RETROSPECT_MARKDOWN: &str = include_str!("../../libs/ak/src/skills/retrospect.v1.md");

const AUTOPILOT_ONE_LINER: &str = r#"stakpak autopilot schedule add --name retrospect --cron "0 3 * * *" --prompt "$(stakpak ak skill retrospect)""#;

#[test]
fn ak_skill_retrospect_prints_bundled_prompt() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let home = temp_dir.path();

    let output = Command::new(env!("CARGO_BIN_EXE_stakpak"))
        .arg("ak")
        .arg("skill")
        .arg("retrospect")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("STAKPAK_PROFILE")
        .output()
        .expect("run stakpak ak skill retrospect");

    assert!(
        output.status.success(),
        "ak skill retrospect failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    // `println!` appends a trailing newline after the content; the bundled
    // file may or may not end with one, so compare on a trim-end basis.
    assert_eq!(
        stdout.trim_end_matches('\n'),
        RETROSPECT_MARKDOWN.trim_end_matches('\n'),
        "stakpak ak skill retrospect output does not match bundled retrospect.v1.md"
    );
}

#[test]
fn readme_autopilot_one_liner_matches_retrospect_prompt() {
    // Prevent drift between the autopilot docs and the one-liner embedded
    // in the skill prompt itself.
    assert!(
        RETROSPECT_MARKDOWN.contains(AUTOPILOT_ONE_LINER),
        "bundled retrospect.v1.md must contain the canonical autopilot one-liner"
    );

    let root_readme = include_str!("../../README.md");
    let cli_readme = include_str!("../README.md");
    assert!(
        root_readme.contains(AUTOPILOT_ONE_LINER) || cli_readme.contains(AUTOPILOT_ONE_LINER),
        "the canonical `autopilot schedule add` one-liner must appear in the autopilot docs (README.md or cli/README.md) so the docs and SKILL_RETROSPECT stay in sync"
    );
}

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
