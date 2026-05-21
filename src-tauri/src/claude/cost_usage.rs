use crate::domain::usage::{CostUsageDailyPoint, CostUsageSnapshot};
use crate::error::AppError;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

mod pricing;

use pricing::{claude_cost_usd, normalize_claude_model};

#[derive(Debug, Clone)]
struct ClaudeUsageRow {
    day_key: String,
    model: String,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_create_tokens: i64,
    output_tokens: i64,
    cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
struct DayRange {
    since_key: String,
    until_key: String,
}

pub fn local_claude_logs_exist(claude_home: &Path) -> bool {
    local_claude_logs_exist_in_project_roots(&claude_projects_roots(claude_home))
}

pub fn load_cost_usage_snapshot(
    source_root: PathBuf,
    _force_rescan: bool,
) -> Result<CostUsageSnapshot, AppError> {
    let now = OffsetDateTime::now_utc();
    let range = DayRange::new(now);
    let roots = claude_projects_roots(&source_root);
    let rows = scan_roots(&roots, &range)?;
    Ok(build_snapshot(rows, roots, &range, now))
}

impl DayRange {
    fn new(now: OffsetDateTime) -> Self {
        let since = now - Duration::days(29);
        Self {
            since_key: day_key_from_datetime(since),
            until_key: day_key_from_datetime(now),
        }
    }

    fn contains(&self, day_key: &str) -> bool {
        day_key >= self.since_key.as_str() && day_key <= self.until_key.as_str()
    }
}

fn claude_projects_roots(source_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if source_root.exists() {
        roots.push(projects_root_for_config_dir(source_root));
    }
    for root in default_claude_projects_roots() {
        if !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
    roots
}

fn local_claude_logs_exist_in_project_roots(roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| first_jsonl_file(root).is_some())
}

fn default_claude_projects_roots() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("CLAUDE_CONFIG_DIR") {
        let roots: Vec<PathBuf> = raw
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .map(|root| projects_root_for_config_dir(&root))
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }

    let home = dirs_home();
    vec![
        home.join(".config").join("claude").join("projects"),
        home.join(".claude").join("projects"),
    ]
}

fn projects_root_for_config_dir(root: &Path) -> PathBuf {
    if root.file_name().and_then(|name| name.to_str()) == Some("projects") {
        root.to_path_buf()
    } else {
        root.join("projects")
    }
}

fn scan_roots(roots: &[PathBuf], range: &DayRange) -> Result<Vec<ClaudeUsageRow>, AppError> {
    let mut keyed_rows = HashMap::<String, ClaudeUsageRow>::new();
    let mut unkeyed_rows = Vec::new();
    for root in roots {
        for file in list_jsonl_files(root)? {
            parse_file(&file, range, &mut keyed_rows, &mut unkeyed_rows)?;
        }
    }
    let mut rows: Vec<ClaudeUsageRow> = keyed_rows.into_values().collect();
    rows.extend(unkeyed_rows);
    Ok(rows)
}

fn parse_file(
    file: &Path,
    range: &DayRange,
    keyed_rows: &mut HashMap<String, ClaudeUsageRow>,
    unkeyed_rows: &mut Vec<ClaudeUsageRow>,
) -> Result<(), AppError> {
    let file = File::open(file).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        if !line.contains(r#""type":"assistant""#) && !line.contains(r#""type": "assistant""#) {
            continue;
        }
        if !line.contains(r#""usage""#) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(row) = parse_usage_row(&value, range) else {
            continue;
        };
        let message_id = value
            .pointer("/message/id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let request_id = value
            .get("requestId")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let (Some(message_id), Some(request_id)) = (message_id, request_id) {
            keyed_rows.insert(format!("{message_id}:{request_id}"), row);
        } else {
            unkeyed_rows.push(row);
        }
    }
    Ok(())
}

fn parse_usage_row(value: &Value, range: &DayRange) -> Option<ClaudeUsageRow> {
    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let timestamp = value.get("timestamp")?.as_str()?;
    let day_key = day_key_from_timestamp(timestamp)?;
    if !range.contains(&day_key) {
        return None;
    }

    let message = value.get("message")?;
    let model = message.get("model")?.as_str()?;
    let usage = message.get("usage")?;
    let input_tokens = token_value(usage, "input_tokens");
    let cache_read_tokens = token_value(usage, "cache_read_input_tokens");
    let cache_create_tokens = token_value(usage, "cache_creation_input_tokens");
    let output_tokens = token_value(usage, "output_tokens");
    if input_tokens == 0 && cache_read_tokens == 0 && cache_create_tokens == 0 && output_tokens == 0
    {
        return None;
    }
    let normalized_model = normalize_claude_model(model);
    let cost_usd = claude_cost_usd(
        &normalized_model,
        input_tokens,
        cache_read_tokens,
        cache_create_tokens,
        output_tokens,
    );

    Some(ClaudeUsageRow {
        day_key,
        model: normalized_model,
        input_tokens,
        cache_read_tokens,
        cache_create_tokens,
        output_tokens,
        cost_usd,
    })
}

fn token_value(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0).max(0)
}

fn build_snapshot(
    rows: Vec<ClaudeUsageRow>,
    roots: Vec<PathBuf>,
    range: &DayRange,
    now: OffsetDateTime,
) -> CostUsageSnapshot {
    let mut by_day = BTreeMap::<String, DailyTotals>::new();
    for row in rows {
        let day = by_day.entry(row.day_key).or_default();
        day.input_tokens += row.input_tokens;
        day.cached_input_tokens += row.cache_read_tokens + row.cache_create_tokens;
        day.output_tokens += row.output_tokens;
        match row.cost_usd {
            Some(cost) => {
                day.cost_usd += cost;
                day.has_priced_cost = true;
            }
            None => {
                day.has_unpriced_cost = true;
            }
        }
        let _ = row.model;
    }

    let mut daily = Vec::new();
    let mut cursor = range.since_key.clone();
    while cursor <= range.until_key {
        let totals = by_day.remove(&cursor).unwrap_or_default();
        daily.push(CostUsageDailyPoint {
            day_key: cursor.clone(),
            input_tokens: totals.input_tokens,
            cached_input_tokens: totals.cached_input_tokens,
            output_tokens: totals.output_tokens,
            total_tokens: totals.input_tokens + totals.cached_input_tokens + totals.output_tokens,
            cost_usd: totals.cost_usd(),
        });
        let Some(next) = next_day_key(&cursor) else {
            break;
        };
        cursor = next;
    }

    let today_key = day_key_from_datetime(now);
    let today = daily
        .iter()
        .find(|point| point.day_key == today_key)
        .cloned()
        .unwrap_or(CostUsageDailyPoint {
            day_key: today_key,
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_usd: None,
        });
    let mut total_tokens = 0;
    let mut total_cost = 0.0;
    let mut cost_seen = false;
    let mut unpriced_cost_seen = false;
    for point in &daily {
        total_tokens += point.total_tokens;
        if let Some(cost) = point.cost_usd {
            total_cost += cost;
            cost_seen = true;
        } else if point.total_tokens > 0 {
            unpriced_cost_seen = true;
        }
    }

    CostUsageSnapshot {
        today_tokens: today.total_tokens,
        today_cost_usd: today.cost_usd,
        last_30_days_tokens: total_tokens,
        last_30_days_cost_usd: (!unpriced_cost_seen && cost_seen).then_some(total_cost),
        daily,
        updated_at: now.unix_timestamp(),
        source_root: roots
            .into_iter()
            .map(|root| root.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(","),
    }
}

#[derive(Default)]
struct DailyTotals {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    cost_usd: f64,
    has_priced_cost: bool,
    has_unpriced_cost: bool,
}

impl DailyTotals {
    fn cost_usd(&self) -> Option<f64> {
        if self.has_unpriced_cost {
            None
        } else {
            self.has_priced_cost.then_some(self.cost_usd)
        }
    }
}

fn list_jsonl_files(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut files = Vec::new();
    collect_jsonl_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), AppError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AppError::ClaudeAccountStore(error.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        if file_type.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn first_jsonl_file(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if let Some(found) = first_jsonl_file(&path) {
                return Some(found);
            }
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            return Some(path);
        }
    }
    None
}

fn day_key_from_timestamp(value: &str) -> Option<String> {
    if value.len() >= 10 && value.as_bytes().get(4) == Some(&b'-') {
        return Some(value[..10].to_string());
    }
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(day_key_from_datetime)
}

fn day_key_from_datetime(value: OffsetDateTime) -> String {
    let date = value.date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn next_day_key(day_key: &str) -> Option<String> {
    let mut parts = day_key.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    let date = time::Date::from_calendar_date(year, month.try_into().ok()?, day).ok()?;
    let next = date.next_day()?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        next.year(),
        u8::from(next.month()),
        next.day()
    ))
}

fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn dedupes_streaming_chunks_by_message_and_request() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let projects = root.join("projects").join("session");
        fs::create_dir_all(&projects).unwrap();
        fs::write(
            projects.join("session.jsonl"),
            r#"{"type":"assistant","timestamp":"2026-05-19T08:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"cache_read_input_tokens":1,"cache_creation_input_tokens":2,"output_tokens":3}}}
{"type":"assistant","timestamp":"2026-05-19T08:00:01Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":20,"cache_read_input_tokens":2,"cache_creation_input_tokens":4,"output_tokens":6}}}
"#,
        )
        .unwrap();

        let snapshot = load_cost_usage_snapshot(root.clone(), true).unwrap();

        let total = snapshot
            .daily
            .iter()
            .find(|point| point.day_key == "2026-05-19")
            .unwrap();
        assert_eq!(total.input_tokens, 20);
        assert_eq!(total.cached_input_tokens, 6);
        assert_eq!(total.output_tokens, 6);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mixed_known_and_unknown_models_keep_day_unpriced() {
        let range = DayRange {
            since_key: "2026-05-19".to_string(),
            until_key: "2026-05-19".to_string(),
        };
        let now = OffsetDateTime::parse("2026-05-19T12:00:00Z", &Rfc3339).unwrap();
        let priced_cost = claude_cost_usd("claude-sonnet-4-5", 100, 0, 0, 10).unwrap();
        let snapshot = build_snapshot(
            vec![
                ClaudeUsageRow {
                    day_key: "2026-05-19".to_string(),
                    model: "claude-sonnet-4-5".to_string(),
                    input_tokens: 100,
                    cache_read_tokens: 0,
                    cache_create_tokens: 0,
                    output_tokens: 10,
                    cost_usd: Some(priced_cost),
                },
                ClaudeUsageRow {
                    day_key: "2026-05-19".to_string(),
                    model: "claude-future-6".to_string(),
                    input_tokens: 50,
                    cache_read_tokens: 0,
                    cache_create_tokens: 0,
                    output_tokens: 5,
                    cost_usd: None,
                },
            ],
            Vec::new(),
            &range,
            now,
        );

        assert_eq!(snapshot.today_tokens, 165);
        assert_eq!(snapshot.daily[0].cost_usd, None);
        assert_eq!(snapshot.today_cost_usd, None);
        assert_eq!(snapshot.last_30_days_cost_usd, None);
    }

    #[cfg(unix)]
    #[test]
    fn local_claude_logs_exist_ignores_symlinked_project_directories() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let projects = root.join("projects");
        let linked_target =
            std::env::temp_dir().join(format!("wovo-claude-cost-target-{}", Uuid::new_v4()));
        fs::create_dir_all(&projects).unwrap();
        fs::create_dir_all(&linked_target).unwrap();
        fs::write(linked_target.join("session.jsonl"), "{}\n").unwrap();
        std::os::unix::fs::symlink(&linked_target, projects.join("linked")).unwrap();

        assert!(!local_claude_logs_exist_in_project_roots(&[projects]));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(linked_target);
    }

    #[test]
    fn local_claude_logs_exist_checks_all_project_roots() {
        let empty_root =
            std::env::temp_dir().join(format!("wovo-claude-cost-empty-{}", Uuid::new_v4()));
        let populated_root =
            std::env::temp_dir().join(format!("wovo-claude-cost-populated-{}", Uuid::new_v4()));
        fs::create_dir_all(&empty_root).unwrap();
        fs::create_dir_all(populated_root.join("session")).unwrap();
        fs::write(populated_root.join("session").join("session.jsonl"), "{}\n").unwrap();

        assert!(local_claude_logs_exist_in_project_roots(&[
            empty_root.clone(),
            populated_root.clone(),
        ]));

        let _ = fs::remove_dir_all(empty_root);
        let _ = fs::remove_dir_all(populated_root);
    }

    #[cfg(unix)]
    #[test]
    fn list_jsonl_files_ignores_symlinked_directories() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let linked_target =
            std::env::temp_dir().join(format!("wovo-claude-cost-target-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&linked_target).unwrap();
        fs::write(linked_target.join("session.jsonl"), "{}\n").unwrap();
        std::os::unix::fs::symlink(&linked_target, root.join("linked")).unwrap();

        assert!(list_jsonl_files(&root).unwrap().is_empty());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(linked_target);
    }
}
