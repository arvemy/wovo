use crate::error::AppError;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const SETTINGS_SCHEMA_JSON: &str = include_str!("../../wovo-settings.schema.json");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WovoConfigValidationReport {
    pub valid: bool,
    pub issues: Vec<WovoConfigValidationIssue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WovoConfigValidationIssue {
    pub file: String,
    pub path: String,
    pub message: String,
}

#[tauri::command]
pub(crate) fn validate_wovo_config() -> Result<WovoConfigValidationReport, AppError> {
    validate_wovo_config_at(&dirs_home().join(".wovo"))
}

fn validate_wovo_config_at(root: &Path) -> Result<WovoConfigValidationReport, AppError> {
    let schema: Value = serde_json::from_str(SETTINGS_SCHEMA_JSON)
        .map_err(|error| AppError::AccountStore(format!("settings schema is invalid: {error}")))?;
    let schema_defs = schema
        .get("$defs")
        .ok_or_else(|| AppError::AccountStore("settings schema is missing $defs".to_string()))?;

    let targets = [
        (
            root.join("codex").join("codex-settings.json"),
            schema_defs.get("providerSettings").ok_or_else(|| {
                AppError::AccountStore("schema is missing providerSettings".to_string())
            })?,
        ),
        (
            root.join("claude").join("claude-settings.json"),
            schema_defs.get("providerSettings").ok_or_else(|| {
                AppError::AccountStore("schema is missing providerSettings".to_string())
            })?,
        ),
        (
            root.join("codex").join("provider-state.json"),
            schema_defs.get("providerState").ok_or_else(|| {
                AppError::AccountStore("schema is missing providerState".to_string())
            })?,
        ),
        (
            root.join("claude").join("provider-state.json"),
            schema_defs.get("providerState").ok_or_else(|| {
                AppError::AccountStore("schema is missing providerState".to_string())
            })?,
        ),
    ];

    let mut issues = Vec::new();
    for (path, target_schema) in targets {
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                issues.push(issue(&path, "$", format!("could not read file: {error}")));
                continue;
            }
        };
        let value: Value = match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(error) => {
                issues.push(issue(&path, "$", format!("invalid JSON: {error}")));
                continue;
            }
        };
        validate_value(&path, "$", &value, target_schema, &schema, &mut issues);
    }

    Ok(WovoConfigValidationReport {
        valid: issues.is_empty(),
        issues,
    })
}

fn validate_value(
    file: &Path,
    path: &str,
    value: &Value,
    schema: &Value,
    root_schema: &Value,
    issues: &mut Vec<WovoConfigValidationIssue>,
) {
    let schema = resolve_ref(schema, root_schema).unwrap_or(schema);

    if let Some(expected_type) = schema.get("type").and_then(Value::as_str) {
        if !value_matches_type(value, expected_type) {
            issues.push(issue(
                file,
                path,
                format!("expected {expected_type}, got {}", value_type(value)),
            ));
            return;
        }
    }

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|allowed| allowed == value) {
            issues.push(issue(
                file,
                path,
                "value is not one of the allowed schema values",
            ));
        }
    }

    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
            if number < minimum {
                issues.push(issue(
                    file,
                    path,
                    format!("value is below minimum {minimum}"),
                ));
            }
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
            if number > maximum {
                issues.push(issue(
                    file,
                    path,
                    format!("value is above maximum {maximum}"),
                ));
            }
        }
    }

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        if let Some(object) = value.as_object() {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    issues.push(issue(
                        file,
                        format!("{path}.{field}"),
                        "required field is missing",
                    ));
                }
            }
        }
    }

    if let Some(array) = value.as_array() {
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate_value(
                    file,
                    &format!("{path}[{index}]"),
                    item,
                    item_schema,
                    root_schema,
                    issues,
                );
            }
        }
    }

    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let Some(object) = value.as_object() else {
        return;
    };
    for (field, field_schema) in properties {
        if let Some(field_value) = object.get(field) {
            validate_value(
                file,
                &format!("{path}.{field}"),
                field_value,
                field_schema,
                root_schema,
                issues,
            );
        }
    }
}

fn resolve_ref<'a>(schema: &'a Value, root_schema: &'a Value) -> Option<&'a Value> {
    let reference = schema.get("$ref")?.as_str()?;
    let name = reference.strip_prefix("#/$defs/")?;
    root_schema.get("$defs")?.get(name)
}

fn value_matches_type(value: &Value, expected_type: &str) -> bool {
    match expected_type {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        _ => true,
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
            "integer"
        }
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn issue(
    file: &Path,
    path: impl Into<String>,
    message: impl Into<String>,
) -> WovoConfigValidationIssue {
    WovoConfigValidationIssue {
        file: file.to_string_lossy().to_string(),
        path: path.into(),
        message: message.into(),
    }
}

fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn validates_settings_against_schema_input() {
        let root = std::env::temp_dir().join(format!("wovo-config-validation-{}", Uuid::new_v4()));
        let codex = root.join("codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("codex-settings.json"),
            r#"{
                "schemaVersion": 2,
                "usageSourceMode": "auto",
                "costUsageEnabled": true,
                "notificationsEnabled": true,
                "autoAccountSwitchingEnabled": false,
                "autoSwitchThresholdPercent": 90,
                "weeklyPenaltyThresholdPercent": 20,
                "costUsageRangeDays": 30,
                "hideAccountCredentials": false,
                "launchOnLogin": false
            }"#,
        )
        .unwrap();

        let report = validate_wovo_config_at(&root).unwrap();

        assert!(report.valid, "{:?}", report.issues);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_settings_without_new_fields_validate_clean() {
        let root = std::env::temp_dir().join(format!("wovo-config-legacy-{}", Uuid::new_v4()));
        let codex = root.join("codex");
        fs::create_dir_all(&codex).unwrap();
        // Pre-rewrite settings only carried usageSourceMode + costUsageEnabled;
        // the loader fills the rest with defaults, so validation must not flag
        // their absence as an error.
        fs::write(
            codex.join("codex-settings.json"),
            r#"{
                "usageSourceMode": "oauth",
                "costUsageEnabled": true
            }"#,
        )
        .unwrap();

        let report = validate_wovo_config_at(&root).unwrap();

        assert!(report.valid, "{:?}", report.issues);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn array_items_with_wrong_type_report_schema_issue() {
        let root = std::env::temp_dir().join(format!("wovo-config-array-{}", Uuid::new_v4()));
        let codex = root.join("codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("codex-settings.json"),
            r#"{
                "schemaVersion": 2,
                "usageSourceMode": "auto",
                "costUsageEnabled": true,
                "configWarnings": [42]
            }"#,
        )
        .unwrap();

        let report = validate_wovo_config_at(&root).unwrap();

        assert!(!report.valid, "{:?}", report.issues);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.path == "$.configWarnings[0]"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_settings_report_schema_issue() {
        let root = std::env::temp_dir().join(format!("wovo-config-validation-{}", Uuid::new_v4()));
        let codex = root.join("codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("codex-settings.json"),
            r#"{
                "schemaVersion": 2,
                "usageSourceMode": "bad",
                "costUsageEnabled": true,
                "notificationsEnabled": true,
                "autoAccountSwitchingEnabled": false,
                "autoSwitchThresholdPercent": 120,
                "weeklyPenaltyThresholdPercent": 20,
                "costUsageRangeDays": 14,
                "hideAccountCredentials": false
            }"#,
        )
        .unwrap();

        let report = validate_wovo_config_at(&root).unwrap();

        assert!(!report.valid);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.path == "$.usageSourceMode"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.path == "$.autoSwitchThresholdPercent"));
        let _ = fs::remove_dir_all(root);
    }
}
