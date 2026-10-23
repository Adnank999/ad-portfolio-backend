use super::dto::{GenerationCost, GenerationUsage};
use serde::Deserialize;
use serde_json::Value;
use std::env;

#[derive(Clone)]
pub struct AiPricing {
    pub input: f64,
    pub output: f64,
    pub cached_input: f64,
}

impl AiPricing {
    pub fn for_model(model: &str) -> Option<Self> {
        (model == "gemini-3.5-flash-lite").then_some(Self {
            input: 0.30,
            output: 2.50,
            cached_input: 0.03,
        })
    }

    pub fn from_env(model: &str) -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let values = [
            "AI_INPUT_USD_PER_MILLION_TOKENS",
            "AI_OUTPUT_USD_PER_MILLION_TOKENS",
            "AI_CACHED_INPUT_USD_PER_MILLION_TOKENS",
        ]
        .map(|name| env::var(name).ok());
        Self::configured(model, values)
    }

    fn configured(
        model: &str,
        values: [Option<String>; 3],
    ) -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        if values.iter().all(Option::is_none) {
            return Ok(Self::for_model(model));
        }
        let mut rates = Vec::new();
        for value in values {
            let rate = value
                .ok_or("Set all three AI token price overrides together")?
                .parse::<f64>()?;
            if !rate.is_finite() || !(0.0..=1_000_000.0).contains(&rate) {
                return Err("AI token prices must be finite, non-negative USD amounts".into());
            }
            rates.push(rate);
        }
        Ok(Some(Self {
            input: rates[0],
            output: rates[1],
            cached_input: rates[2],
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUsage {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
    #[serde(default)]
    thoughts_token_count: u32,
    #[serde(default)]
    cached_content_token_count: u32,
}

pub fn from_envelope(
    envelope: &Value,
    model: &str,
    pricing: Option<&AiPricing>,
) -> Option<GenerationUsage> {
    let usage: ProviderUsage =
        serde_json::from_value(envelope.get("usageMetadata")?.clone()).ok()?;
    let output_tokens = usage
        .candidates_token_count
        .checked_add(usage.thoughts_token_count)?;
    let accounted = usage.prompt_token_count.checked_add(output_tokens)?;
    if usage.cached_content_token_count > usage.prompt_token_count
        || usage.total_token_count < accounted
    {
        return None;
    }
    let estimated_cost = pricing
        .filter(|_| usage.total_token_count == accounted)
        .map(|rates| {
            let input_usd =
                (f64::from(usage.prompt_token_count - usage.cached_content_token_count)
                    * rates.input
                    + f64::from(usage.cached_content_token_count) * rates.cached_input)
                    / 1_000_000.0;
            let output_usd = f64::from(output_tokens) * rates.output / 1_000_000.0;
            GenerationCost {
                currency: "USD".into(),
                input_usd,
                output_usd,
                total_usd: input_usd + output_usd,
                input_usd_per_million: rates.input,
                output_usd_per_million: rates.output,
                cached_input_usd_per_million: rates.cached_input,
            }
        });
    Some(GenerationUsage {
        model: model.into(),
        input_tokens: usage.prompt_token_count,
        output_tokens,
        thinking_tokens: usage.thoughts_token_count,
        cached_input_tokens: usage.cached_content_token_count,
        total_tokens: usage.total_token_count,
        estimated_cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope() -> Value {
        json!({"usageMetadata":{"promptTokenCount":1000,"cachedContentTokenCount":200,"candidatesTokenCount":400,"thoughtsTokenCount":100,"totalTokenCount":1500}})
    }

    #[test]
    fn calculates_uncached_cached_and_thinking_cost_without_double_counting() {
        let usage = from_envelope(
            &envelope(),
            "gemini-3.5-flash-lite",
            AiPricing::for_model("gemini-3.5-flash-lite").as_ref(),
        )
        .unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.thinking_tokens, 100);
        assert_eq!(usage.total_tokens, 1500);
        let cost = usage.estimated_cost.as_ref().unwrap();
        assert!((cost.input_usd - 0.000246).abs() < 1e-12);
        assert!((cost.output_usd - 0.00125).abs() < 1e-12);
        assert!((cost.total_usd - 0.001496).abs() < 1e-12);
        let encoded = serde_json::to_value(&usage).unwrap();
        assert_eq!(encoded["estimatedCost"]["currency"], "USD");
        assert_eq!(encoded["thinkingTokens"], 100);
    }

    #[test]
    fn missing_invalid_or_unpriced_usage_never_fabricates_a_zero_cost() {
        assert!(from_envelope(&json!({}), "unknown", None).is_none());
        for metadata in [
            json!({"promptTokenCount":-1,"candidatesTokenCount":2,"totalTokenCount":3}),
            json!({"promptTokenCount":1,"candidatesTokenCount":2,"totalTokenCount":3,"cachedContentTokenCount":2}),
            json!({"promptTokenCount":1,"candidatesTokenCount":2,"totalTokenCount":2}),
            json!({"promptTokenCount":1}),
        ] {
            assert!(from_envelope(&json!({"usageMetadata":metadata}), "unknown", None).is_none());
        }
        let usage = from_envelope(&envelope(), "another-model", None).unwrap();
        assert!(usage.estimated_cost.is_none());
        let mut extra = envelope();
        extra["usageMetadata"]["totalTokenCount"] = json!(1600);
        assert!(from_envelope(
            &extra,
            "gemini-3.5-flash-lite",
            AiPricing::for_model("gemini-3.5-flash-lite").as_ref()
        )
        .unwrap()
        .estimated_cost
        .is_none());
    }

    #[test]
    fn price_overrides_are_complete_valid_and_allow_free_tier_rates() {
        assert!(AiPricing::configured("other", [None, None, None])
            .unwrap()
            .is_none());
        assert!(AiPricing::configured("other", [Some("1".into()), None, None]).is_err());
        for invalid in ["NaN", "inf", "-1", "not-a-number"] {
            assert!(AiPricing::configured(
                "other",
                [Some(invalid.into()), Some("2".into()), Some("0.1".into())]
            )
            .is_err());
        }
        let pricing = AiPricing::configured(
            "other",
            [Some("0".into()), Some("0".into()), Some("0".into())],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            from_envelope(&envelope(), "other", Some(&pricing))
                .unwrap()
                .estimated_cost
                .unwrap()
                .total_usd,
            0.0
        );
        let usage = from_envelope(&json!({"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":4,"totalTokenCount":14}}), "other", Some(&pricing)).unwrap();
        assert_eq!(usage.thinking_tokens, 0);
        assert_eq!(usage.cached_input_tokens, 0);
    }
}
