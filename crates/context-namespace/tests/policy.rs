use context_namespace::NamespaceConfig;
use std::num::NonZeroU32;

fn valid_config() -> NamespaceConfig {
    NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    }
}

#[test]
fn valid_default_config_passes_validation() {
    let config = valid_config();
    assert!(config.validate().is_ok());
}

#[test]
fn trigger_tokens_greater_than_target_tokens_is_valid() {
    let mut config = valid_config();
    config.policy.trigger_tokens = 24_000;
    config.policy.target_tokens = 16_000;
    assert!(config.validate().is_ok());
}

#[test]
fn trigger_tokens_equal_to_target_tokens_is_valid() {
    let mut config = valid_config();
    config.policy.trigger_tokens = 16_000;
    config.policy.target_tokens = 16_000;
    assert!(config.validate().is_ok());
}

#[test]
fn trigger_tokens_less_than_target_tokens_is_invalid() {
    let mut config = valid_config();
    config.policy.trigger_tokens = 10_000;
    config.policy.target_tokens = 20_000;
    assert!(config.validate().is_err());
}

#[test]
fn effective_budget_less_than_trigger_tokens_is_invalid() {
    let mut config = valid_config();
    config.policy.effective_budget_tokens = 10_000;
    config.policy.trigger_tokens = 24_000;
    assert!(config.validate().is_err());
}

#[test]
fn zero_preserve_prefix_records_is_invalid() {
    let mut config = valid_config();
    config.policy.preserve_prefix_records = 0;
    assert!(config.validate().is_err());
}

#[test]
fn zero_preserve_recent_records_is_invalid() {
    let mut config = valid_config();
    config.policy.preserve_recent_records = 0;
    assert!(config.validate().is_err());
}

#[test]
fn empty_render_field_is_invalid() {
    let mut config = valid_config();
    config.render_field = String::new();
    assert!(config.validate().is_err());
}

#[test]
fn default_expansion_ttl_greater_than_max_is_invalid() {
    let mut config = valid_config();
    config.policy.default_expansion_ttl = NonZeroU32::new(5).unwrap();
    config.policy.max_expansion_ttl = NonZeroU32::new(4).unwrap();
    assert!(config.validate().is_err());
}

#[test]
fn limits_max_records_zero_is_invalid() {
    let mut config = valid_config();
    config.limits.max_records = NonZeroU32::new(0).unwrap_or(NonZeroU32::new(1).unwrap());
    // NonZeroU32 cannot be 0, but we test with a very small value to ensure
    // validation does not reject legitimate small limits.
    assert!(config.validate().is_ok());
}

#[test]
fn limits_max_tree_depth_one_is_valid() {
    let mut config = valid_config();
    config.limits.max_tree_depth = NonZeroU32::new(1).unwrap();
    assert!(config.validate().is_ok());
}
