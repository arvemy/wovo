#[derive(Debug, Clone, Copy)]
struct ClaudePricing {
    input_cost_per_token: f64,
    output_cost_per_token: f64,
    cache_creation_input_cost_per_token: f64,
    cache_read_input_cost_per_token: f64,
    threshold_tokens: Option<i64>,
    input_cost_per_token_above_threshold: Option<f64>,
    output_cost_per_token_above_threshold: Option<f64>,
    cache_creation_input_cost_per_token_above_threshold: Option<f64>,
    cache_read_input_cost_per_token_above_threshold: Option<f64>,
}

pub(super) fn claude_cost_usd(
    model: &str,
    input_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
) -> Option<f64> {
    let pricing = claude_pricing(&normalize_claude_model(model))?;
    Some(
        tiered(
            input_tokens.max(0),
            pricing.input_cost_per_token,
            pricing.input_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ) + tiered(
            cache_read_input_tokens.max(0),
            pricing.cache_read_input_cost_per_token,
            pricing.cache_read_input_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ) + tiered(
            cache_creation_input_tokens.max(0),
            pricing.cache_creation_input_cost_per_token,
            pricing.cache_creation_input_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ) + tiered(
            output_tokens.max(0),
            pricing.output_cost_per_token,
            pricing.output_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ),
    )
}

fn tiered(tokens: i64, base: f64, above: Option<f64>, threshold: Option<i64>) -> f64 {
    let Some(threshold) = threshold else {
        return tokens as f64 * base;
    };
    let Some(above) = above else {
        return tokens as f64 * base;
    };
    let below = tokens.min(threshold);
    let over = (tokens - threshold).max(0);
    below as f64 * base + over as f64 * above
}

fn claude_pricing(model: &str) -> Option<ClaudePricing> {
    let pricing = match model {
        "claude-haiku-4-5-20251001" | "claude-haiku-4-5" => ClaudePricing {
            input_cost_per_token: 1e-6,
            output_cost_per_token: 5e-6,
            cache_creation_input_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: 1e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
        "claude-opus-4-5-20251101"
        | "claude-opus-4-5"
        | "claude-opus-4-6-20260205"
        | "claude-opus-4-6"
        | "claude-opus-4-7" => ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
        "claude-sonnet-4-5" | "claude-sonnet-4-6" | "claude-sonnet-4-5-20250929" => ClaudePricing {
            input_cost_per_token: 3e-6,
            output_cost_per_token: 1.5e-5,
            cache_creation_input_cost_per_token: 3.75e-6,
            cache_read_input_cost_per_token: 3e-7,
            threshold_tokens: Some(200_000),
            input_cost_per_token_above_threshold: Some(6e-6),
            output_cost_per_token_above_threshold: Some(2.25e-5),
            cache_creation_input_cost_per_token_above_threshold: Some(7.5e-6),
            cache_read_input_cost_per_token_above_threshold: Some(6e-7),
        },
        "claude-opus-4-20250514" | "claude-opus-4-1" => ClaudePricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 7.5e-5,
            cache_creation_input_cost_per_token: 1.875e-5,
            cache_read_input_cost_per_token: 1.5e-6,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
        "claude-sonnet-4-20250514" => ClaudePricing {
            input_cost_per_token: 3e-6,
            output_cost_per_token: 1.5e-5,
            cache_creation_input_cost_per_token: 3.75e-6,
            cache_read_input_cost_per_token: 3e-7,
            threshold_tokens: Some(200_000),
            input_cost_per_token_above_threshold: Some(6e-6),
            output_cost_per_token_above_threshold: Some(2.25e-5),
            cache_creation_input_cost_per_token_above_threshold: Some(7.5e-6),
            cache_read_input_cost_per_token_above_threshold: Some(6e-7),
        },
        _ => return None,
    };
    Some(pricing)
}

pub(super) fn normalize_claude_model(raw: &str) -> String {
    let mut trimmed = raw.trim().to_string();
    if let Some(rest) = trimmed.strip_prefix("anthropic.") {
        trimmed = rest.to_string();
    }
    if let Some(index) = trimmed.rfind('.') {
        let tail = &trimmed[index + 1..];
        if tail.starts_with("claude-") {
            trimmed = tail.to_string();
        }
    }
    if let Some(index) = trimmed.find("-v") {
        let suffix = &trimmed[index..];
        if suffix
            .chars()
            .skip(2)
            .all(|ch| ch.is_ascii_digit() || ch == ':')
        {
            trimmed.truncate(index);
        }
    }
    if trimmed.len() > 9 {
        let suffix = &trimmed[trimmed.len() - 9..];
        if suffix.starts_with('-') && suffix[1..].chars().all(|ch| ch.is_ascii_digit()) {
            let base = &trimmed[..trimmed.len() - 9];
            if claude_pricing(base).is_some() {
                return base.to_string();
            }
        }
    }
    trimmed
}
