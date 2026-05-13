use super::*;
use uuid::Uuid;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("wovo-cost-{name}-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_session(root: &Path, day_key: &str, filename: &str, lines: &[Value]) -> PathBuf {
    let parts: Vec<&str> = day_key.split('-').collect();
    let dir = root
        .join("sessions")
        .join(parts[0])
        .join(parts[1])
        .join(parts[2]);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(filename);
    fs::write(&path, jsonl(lines)).unwrap();
    path
}

fn write_archived(root: &Path, filename: &str, lines: &[Value]) -> PathBuf {
    let dir = root.join("archived_sessions");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(filename);
    fs::write(&path, jsonl(lines)).unwrap();
    path
}

fn jsonl(lines: &[Value]) -> String {
    lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-04-12T12:00:00Z", &Rfc3339).unwrap()
}

fn token_total(timestamp: &str, model: &str, input: i64, cached: i64, output: i64) -> Value {
    serde_json::json!({
        "type": "event_msg",
        "timestamp": timestamp,
        "payload": {
            "type": "token_count",
            "info": {
                "model": model,
                "total_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output
                }
            }
        }
    })
}

fn token_last(timestamp: &str, model: &str, input: i64, cached: i64, output: i64) -> Value {
    serde_json::json!({
        "type": "event_msg",
        "timestamp": timestamp,
        "payload": {
            "type": "token_count",
            "info": {
                "model": model,
                "last_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output
                }
            }
        }
    })
}

fn session_meta(session_id: &str, forked_from_id: Option<&str>, timestamp: &str) -> Value {
    let mut payload = serde_json::json!({ "session_id": session_id });
    if let Some(forked_from_id) = forked_from_id {
        payload["forked_from_id"] = Value::String(forked_from_id.to_string());
    }
    serde_json::json!({
        "type": "session_meta",
        "timestamp": timestamp,
        "payload": payload
    })
}

#[test]
fn parses_total_token_usage_deltas() {
    let root = temp_root("total-deltas");
    let cache = root.join("cache");
    write_session(
        &root,
        "2026-04-12",
        "session.jsonl",
        &[
            token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10),
            token_total("2026-04-12T10:01:00Z", "gpt-5.4", 160, 30, 16),
        ],
    );

    let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(snapshot.today_tokens, 176);
    assert_eq!(snapshot.daily[0].input_tokens, 160);
    assert_eq!(snapshot.daily[0].cached_input_tokens, 30);
    assert_eq!(snapshot.daily[0].output_tokens, 16);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn parses_last_token_usage_rows() {
    let root = temp_root("last-rows");
    let cache = root.join("cache");
    write_session(
        &root,
        "2026-04-12",
        "session.jsonl",
        &[
            token_last("2026-04-12T10:00:00Z", "gpt-5.4", 50, 5, 3),
            token_last("2026-04-12T10:01:00Z", "gpt-5.4", 70, 7, 5),
        ],
    );

    let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(snapshot.today_tokens, 128);
    assert_eq!(snapshot.daily[0].input_tokens, 120);
    assert_eq!(snapshot.daily[0].cached_input_tokens, 12);
    assert_eq!(snapshot.daily[0].output_tokens, 8);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn turn_context_model_wins_over_token_payload_model() {
    let root = temp_root("turn-context");
    let cache = root.join("cache");
    write_session(
        &root,
        "2026-04-12",
        "session.jsonl",
        &[
            serde_json::json!({
                "type": "turn_context",
                "timestamp": "2026-04-12T09:59:59Z",
                "payload": { "model": "openai/gpt-5.4" }
            }),
            token_total("2026-04-12T10:00:00Z", "gpt-5", 100, 20, 10),
        ],
    );

    let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();
    let expected = codex_cost_usd("gpt-5.4", 100, 20, 10).unwrap();

    assert!((snapshot.today_cost_usd.unwrap() - expected).abs() < 0.000001);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cached_input_tokens_use_cache_read_pricing() {
    let priced = codex_cost_usd("gpt-5.4", 100, 20, 10).unwrap();
    let uncached = codex_cost_usd("gpt-5.4", 100, 0, 10).unwrap();

    assert!(priced < uncached);
}

#[test]
fn scans_archived_flat_session_files() {
    let root = temp_root("archived");
    let cache = root.join("cache");
    write_archived(
        &root,
        "rollout-2026-04-12T10-00-00-session.jsonl",
        &[
            serde_json::json!({
                "type": "session_meta",
                "payload": { "session_id": "archived-session" }
            }),
            token_last("2026-04-12T10:00:00Z", "gpt-5.4", 33, 3, 4),
        ],
    );

    let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(snapshot.today_tokens, 37);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_model_keeps_tokens_with_null_cost() {
    let root = temp_root("unknown-model");
    let cache = root.join("cache");
    write_session(
        &root,
        "2026-04-12",
        "session.jsonl",
        &[token_total(
            "2026-04-12T10:00:00Z",
            "unknown-model",
            100,
            0,
            10,
        )],
    );

    let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(snapshot.today_tokens, 110);
    assert_eq!(snapshot.today_cost_usd, None);
    assert_eq!(snapshot.daily[0].cost_usd, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn incremental_cache_reuses_previous_totals_after_append() {
    let root = temp_root("incremental");
    let cache = root.join("cache");
    let path = write_session(
        &root,
        "2026-04-12",
        "session.jsonl",
        &[token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10)],
    );

    let first = load_cost_usage_snapshot_at(root.clone(), cache.clone(), now(), false).unwrap();
    assert_eq!(first.today_tokens, 110);

    fs::write(
        &path,
        jsonl(&[
            token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10),
            token_total("2026-04-12T10:01:00Z", "gpt-5.4", 150, 30, 15),
        ]),
    )
    .unwrap();

    let second = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(second.today_tokens, 165);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn incremental_cache_reparses_unterminated_tail_after_append() {
    let root = temp_root("incremental-partial-tail");
    let cache = root.join("cache");
    let path = write_session(
        &root,
        "2026-04-12",
        "session.jsonl",
        &[token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10)],
    );
    let first_line = jsonl(&[token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10)]);
    let second_line = token_total("2026-04-12T10:01:00Z", "gpt-5.4", 150, 30, 15).to_string();
    let split_at = second_line.len() / 2;

    fs::write(&path, format!("{first_line}{}", &second_line[..split_at])).unwrap();
    let first = load_cost_usage_snapshot_at(root.clone(), cache.clone(), now(), false).unwrap();
    assert_eq!(first.today_tokens, 110);

    fs::write(&path, format!("{first_line}{second_line}\n")).unwrap();
    let second = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(second.today_tokens, 165);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forked_last_token_usage_rows_are_counted_as_deltas() {
    let root = temp_root("forked-last-rows");
    let cache = root.join("cache");
    write_session(
        &root,
        "2026-04-12",
        "parent.jsonl",
        &[
            session_meta("parent-session", None, "2026-04-12T09:00:00Z"),
            token_total("2026-04-12T10:00:00Z", "gpt-5.4", 1000, 100, 100),
        ],
    );
    write_session(
        &root,
        "2026-04-12",
        "child.jsonl",
        &[
            session_meta(
                "child-session",
                Some("parent-session"),
                "2026-04-12T10:30:00Z",
            ),
            token_last("2026-04-12T10:31:00Z", "gpt-5.4", 100, 10, 10),
        ],
    );

    let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

    assert_eq!(snapshot.today_tokens, 1210);
    assert_eq!(snapshot.daily[0].input_tokens, 1100);
    assert_eq!(snapshot.daily[0].cached_input_tokens, 110);
    assert_eq!(snapshot.daily[0].output_tokens, 110);
    let _ = fs::remove_dir_all(root);
}
