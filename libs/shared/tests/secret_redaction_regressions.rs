//! Regression suite for redaction fixes reported by Abdalla.

use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use regex::Regex;
use stakpak_shared::secrets::{redact_password, redact_secrets, restore_secrets};
use std::collections::HashMap;

fn fake_slack_bot_token_a() -> String {
    [
        "xoxb-",
        "2154536101-1638566032918-",
        "aBcDeFgHiJkLmNoPqRsTuVwX",
    ]
    .concat()
}

fn fake_slack_bot_token_b() -> String {
    [
        "xoxb-",
        "2154536102-1638566032919-",
        "zYxWvUtSrQpOnMlKjIhGfEdC",
    ]
    .concat()
}

fn fake_slack_app_token() -> String {
    [
        "xapp-",
        "1-A012345BCDE-1234567890123-",
        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
    ]
    .concat()
}

fn fake_aws_key() -> String {
    ["AKIA", "IOSFODNN7EXREAL2"].concat()
}

fn marker_regex() -> Regex {
    Regex::new(r"\[REDACTED_SECRET:[^:\]]+:[^:\]]+\]").expect("marker regex should compile")
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

// ─────────────────────────────────────────────────────────────────────────────
// SUSPECT #1: redact_secrets early-returns when input contains [REDACTED_SECRET:
// → any *fresh* secret in the same content is silently NOT redacted.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn regression_redact_single_stale_marker_does_not_suppress_fresh_secret() {
    let aws = fake_aws_key();
    let content = format!(
        "Earlier: [REDACTED_SECRET:slack-bot-token:abc123]\nNewly pasted: AWS_ACCESS_KEY_ID={aws}"
    );

    let result = redact_secrets(&content, None, &HashMap::new(), false);

    let leaked = result.redacted_string.contains(&aws);

    assert!(
        !leaked,
        "BUG REPRODUCED: fresh AWS key was NOT redacted because content already \
         contained an unrelated [REDACTED_SECRET:...] placeholder"
    );
    assert!(
        result
            .redacted_string
            .contains("[REDACTED_SECRET:slack-bot-token:abc123]"),
        "stale marker should remain unchanged"
    );
    assert_eq!(result.redaction_map.len(), 1, "expected one new redaction");
}

#[test]
fn regression_redact_multiple_stale_markers_do_not_suppress_detection() {
    let token = fake_slack_bot_token_a();
    let stale_markers = [
        "[REDACTED_SECRET:slack-bot-token:old111]",
        "[REDACTED_SECRET:slack-app-token:old222]",
        "[REDACTED_SECRET:aws-access-token:old333]",
    ];
    let content = format!(
        "{}\n{}\n{}\nSLACK_BOT_TOKEN={token}",
        stale_markers[0], stale_markers[1], stale_markers[2]
    );

    let result = redact_secrets(&content, None, &HashMap::new(), false);

    assert!(!result.redacted_string.contains(&token));
    for marker in stale_markers {
        assert!(result.redacted_string.contains(marker));
    }
    assert_eq!(result.redaction_map.len(), 1);
}

#[test]
fn regression_redact_lone_marker_round_trips_unchanged() {
    let content = "[REDACTED_SECRET:slack-bot-token:abc123]";

    let result = redact_secrets(content, None, &HashMap::new(), false);

    assert_eq!(result.redacted_string, content);
    assert!(result.redaction_map.is_empty());
}

#[test]
fn regression_redact_session_growth_two_calls_two_secrets() {
    let token_a = fake_slack_bot_token_a();
    let token_b = fake_slack_bot_token_b();

    let first = redact_secrets(
        &format!("SLACK_BOT_TOKEN={token_a}"),
        None,
        &HashMap::new(),
        false,
    );
    let second = redact_secrets(
        &format!("SLACK_BOT_TOKEN={token_b}"),
        None,
        &first.redaction_map,
        false,
    );

    assert!(!first.redacted_string.contains(&token_a));
    assert!(!second.redacted_string.contains(&token_b));
    assert_eq!(first.redaction_map.len(), 1);
    assert_eq!(second.redaction_map.len(), 2);
    assert!(second.redaction_map.values().any(|value| value == &token_a));
    assert!(second.redaction_map.values().any(|value| value == &token_b));
}

#[test]
fn regression_redact_identical_secret_across_calls_reuses_key() {
    let token = fake_slack_bot_token_a();

    let first = redact_secrets(
        &format!("SLACK_BOT_TOKEN={token}"),
        None,
        &HashMap::new(),
        false,
    );
    let second = redact_secrets(
        &format!("SLACK_BOT_TOKEN={token}"),
        None,
        &first.redaction_map,
        false,
    );

    let first_key = first
        .redaction_map
        .iter()
        .find(|(_, value)| *value == &token)
        .map(|(key, _)| key.clone())
        .expect("first call should store token");
    let second_key = second
        .redaction_map
        .iter()
        .find(|(_, value)| *value == &token)
        .map(|(key, _)| key.clone())
        .expect("second call should store token");

    assert_eq!(first_key, second_key);
}

#[test]
fn regression_redact_old_map_secret_redacted_at_all_occurrences() {
    let redaction_key = "[REDACTED_SECRET:manual:abc]";
    let secret = "s3cret";
    let old_map = HashMap::from([(redaction_key.to_string(), secret.to_string())]);
    let content = "a=s3cret b=s3cret c=s3cret";

    let result = redact_secrets(content, None, &old_map, false);

    assert_eq!(count_occurrences(&result.redacted_string, redaction_key), 3);
    assert_eq!(count_occurrences(&result.redacted_string, secret), 0);
}

#[test]
fn regression_redact_password_ignores_stale_markers() {
    let password = "supersecret123";
    let content = format!("existing=[REDACTED_SECRET:password:abc123]\npassword={password}");

    let result = redact_password(&content, password, &HashMap::new());

    assert!(!result.redacted_string.contains(password));
    assert!(
        result
            .redacted_string
            .contains("[REDACTED_SECRET:password:abc123]"),
        "stale marker should remain unchanged"
    );
    assert_eq!(result.redaction_map.len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// SUSPECT #2: slack token regex boundaries
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn regression_redact_slack_xoxb_full_token_redacted() {
    let token = fake_slack_bot_token_a();
    let content = format!("SLACK_BOT_TOKEN={token}");

    let result = redact_secrets(&content, None, &HashMap::new(), false);

    assert!(
        !result.redacted_string.contains(&token),
        "xoxb token leaked: {}",
        result.redacted_string
    );
    assert!(
        !result.redaction_map.is_empty(),
        "no redaction recorded for xoxb"
    );
    let stored = result
        .redaction_map
        .values()
        .find(|v| v.starts_with("xoxb-"))
        .expect("no xoxb value in map");
    assert_eq!(stored, &token);
}

#[test]
fn regression_redact_slack_xapp_full_token_redacted() {
    let token = fake_slack_app_token();
    let content = format!("SLACK_APP_TOKEN={token}");

    let result = redact_secrets(&content, None, &HashMap::new(), false);

    assert!(
        !result.redacted_string.contains(&token),
        "xapp token leaked: {}",
        result.redacted_string
    );
    let stored = result
        .redaction_map
        .values()
        .find(|v| v.starts_with("xapp-"))
        .expect("no xapp value in map");
    assert_eq!(stored, &token);
}

#[test]
fn regression_redact_xoxb_full_token_in_grep_style_with_example() {
    let token = fake_slack_bot_token_b();
    let content = format!(".env:SLACK_BOT_TOKEN={token}\n.env.example:SLACK_BOT_TOKEN=xoxb-foo");
    let result = redact_secrets(&content, None, &HashMap::new(), false);

    assert!(
        !result.redacted_string.contains(&token),
        "real xoxb leaked alongside example: {}",
        result.redacted_string
    );
    let stored = result
        .redaction_map
        .values()
        .find(|value| value.starts_with("xoxb-"))
        .expect("no xoxb value in map");
    assert_eq!(stored, &token);
}

// ─────────────────────────────────────────────────────────────────────────────
// SUSPECT #3: shell-special chars in restored secret mangle tool calls
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn regression_restore_preserves_shell_special_chars() {
    let secret = r#"p@ss$word`with"quotes\and$VAR"#;
    let mut map = HashMap::new();
    map.insert(
        "[REDACTED_SECRET:password:abc123]".to_string(),
        secret.to_string(),
    );

    let agent_emitted = "echo \"$TOKEN\" && curl -H 'Authorization: Bearer [REDACTED_SECRET:password:abc123]' https://x";
    let restored = restore_secrets(agent_emitted, &map);

    assert!(
        restored.contains(secret),
        "restored output is missing the original secret characters"
    );
}

#[test]
fn regression_restore_value_looks_like_other_key_no_chain_replace() {
    let mut map = HashMap::new();
    map.insert(
        "[REDACTED_SECRET:a:111]".to_string(),
        "[REDACTED_SECRET:b:222]".to_string(),
    );
    map.insert("[REDACTED_SECRET:b:222]".to_string(), "BAD".to_string());

    let agent_emitted = "value=[REDACTED_SECRET:a:111]";
    let restored = restore_secrets(agent_emitted, &map);

    assert_eq!(
        restored, "value=[REDACTED_SECRET:b:222]",
        "restore_secrets chain-replaced — secret a was rewritten using secret b's mapping"
    );
}

#[test]
fn regression_restore_determinism_50_entries_20_trials() {
    let entries: Vec<(String, String)> = (0..50)
        .map(|index| {
            let key = format!("[REDACTED_SECRET:rule:{index:03}]");
            let value = if index % 10 == 0 && index < 49 {
                format!("[REDACTED_SECRET:rule:{:03}]", index + 1)
            } else {
                format!("secret-value-{index:03}")
            };
            (key, value)
        })
        .collect();
    let redacted = entries
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let expected = entries
        .iter()
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join("|");

    for seed in 0_u64..20 {
        let mut shuffled = entries.clone();
        shuffled.shuffle(&mut StdRng::seed_from_u64(seed));
        let map = shuffled.into_iter().collect::<HashMap<_, _>>();
        let restored = restore_secrets(&redacted, &map);
        assert_eq!(
            restored, expected,
            "restore output drifted for trial {seed}"
        );
    }
}

#[test]
fn regression_restore_adjacent_markers() {
    let map = HashMap::from([
        ("[REDACTED_SECRET:a:111]".to_string(), "X".to_string()),
        ("[REDACTED_SECRET:b:222]".to_string(), "Y".to_string()),
    ]);

    let restored = restore_secrets("[REDACTED_SECRET:a:111][REDACTED_SECRET:b:222]", &map);

    assert_eq!(restored, "XY");
}

#[test]
fn regression_restore_marker_adjacent_to_text() {
    let map = HashMap::from([("[REDACTED_SECRET:a:111]".to_string(), "VALUE".to_string())]);

    let restored = restore_secrets("prefix[REDACTED_SECRET:a:111]suffix", &map);

    assert_eq!(restored, "prefixVALUEsuffix");
}

#[test]
fn regression_restore_unknown_marker_passes_through() {
    let map = HashMap::new();
    let input = "prefix[REDACTED_SECRET:slack-bot-token:unknown999]suffix";

    let restored = restore_secrets(input, &map);

    assert_eq!(restored, input);
}

#[test]
fn regression_marker_generated_redaction_key_matches_regex() {
    let marker_re = marker_regex();

    for index in 0..1000 {
        let password = format!("password-{index}-with-suffix");
        let result = redact_password(&password, &password, &HashMap::new());
        let key = result
            .redaction_map
            .keys()
            .next()
            .expect("redact_password should generate one key");
        assert!(
            marker_re.is_match(key),
            "generated key did not match regex: {key}"
        );
    }
}

#[test]
fn regression_marker_regex_does_not_match_extra_colons() {
    let marker_re = marker_regex();
    let malformed = "[REDACTED_SECRET:a:b:c]";

    assert!(
        marker_re.find(malformed).is_none(),
        "marker regex should ignore malformed markers with extra colons"
    );
}
