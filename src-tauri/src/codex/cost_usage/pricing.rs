#[derive(Debug, Clone, Copy)]
struct CodexPricing {
    input_cost_per_token: f64,
    output_cost_per_token: f64,
    cache_read_input_cost_per_token: Option<f64>,
}

pub(super) fn codex_cost_usd(
    model: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
) -> Option<f64> {
    let pricing = codex_pricing(&normalize_codex_model(model))?;
    let cached = cached_input_tokens.max(0).min(input_tokens.max(0));
    let non_cached = (input_tokens - cached).max(0);
    let cached_rate = pricing
        .cache_read_input_cost_per_token
        .unwrap_or(pricing.input_cost_per_token);
    Some(
        non_cached as f64 * pricing.input_cost_per_token
            + cached as f64 * cached_rate
            + output_tokens.max(0) as f64 * pricing.output_cost_per_token,
    )
}

fn codex_pricing(model: &str) -> Option<CodexPricing> {
    let pricing = match model {
        "gpt-5" | "gpt-5-codex" | "gpt-5.1" | "gpt-5.1-codex" | "gpt-5.1-codex-max" => {
            CodexPricing {
                input_cost_per_token: 1.25e-6,
                output_cost_per_token: 1e-5,
                cache_read_input_cost_per_token: Some(1.25e-7),
            }
        }
        "gpt-5-mini" => CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: Some(2.5e-8),
        },
        "gpt-5-nano" => CodexPricing {
            input_cost_per_token: 5e-8,
            output_cost_per_token: 4e-7,
            cache_read_input_cost_per_token: Some(5e-9),
        },
        "gpt-5-pro" => CodexPricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 1.2e-4,
            cache_read_input_cost_per_token: None,
        },
        "gpt-5.1-codex-mini" => CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: Some(2.5e-8),
        },
        "gpt-5.2" | "gpt-5.2-codex" | "gpt-5.3-codex" => CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: Some(1.75e-7),
        },
        "gpt-5.2-pro" => CodexPricing {
            input_cost_per_token: 2.1e-5,
            output_cost_per_token: 1.68e-4,
            cache_read_input_cost_per_token: None,
        },
        "gpt-5.3-codex-spark" => CodexPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            cache_read_input_cost_per_token: Some(0.0),
        },
        "gpt-5.4" => CodexPricing {
            input_cost_per_token: 2.5e-6,
            output_cost_per_token: 1.5e-5,
            cache_read_input_cost_per_token: Some(2.5e-7),
        },
        "gpt-5.4-mini" => CodexPricing {
            input_cost_per_token: 7.5e-7,
            output_cost_per_token: 4.5e-6,
            cache_read_input_cost_per_token: Some(7.5e-8),
        },
        "gpt-5.4-nano" => CodexPricing {
            input_cost_per_token: 2e-7,
            output_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: Some(2e-8),
        },
        "gpt-5.4-pro" | "gpt-5.5-pro" => CodexPricing {
            input_cost_per_token: 3e-5,
            output_cost_per_token: 1.8e-4,
            cache_read_input_cost_per_token: None,
        },
        "gpt-5.5" => CodexPricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 3e-5,
            cache_read_input_cost_per_token: Some(5e-7),
        },
        _ => return None,
    };
    Some(pricing)
}

pub(super) fn normalize_codex_model(raw: &str) -> String {
    let mut trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("openai/") {
        trimmed = rest;
    }
    if codex_pricing_exact(trimmed) {
        return trimmed.to_string();
    }
    if let Some(base) = strip_dated_suffix(trimmed) {
        if codex_pricing_exact(base) {
            return base.to_string();
        }
    }
    trimmed.to_string()
}

fn codex_pricing_exact(model: &str) -> bool {
    codex_pricing(model).is_some()
}

fn strip_dated_suffix(value: &str) -> Option<&str> {
    if value.len() < 11 {
        return None;
    }
    let suffix = &value[value.len() - 11..];
    let bytes = suffix.as_bytes();
    if bytes[0] == b'-'
        && bytes[1..5].iter().all(u8::is_ascii_digit)
        && bytes[5] == b'-'
        && bytes[6..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..11].iter().all(u8::is_ascii_digit)
    {
        Some(&value[..value.len() - 11])
    } else {
        None
    }
}
