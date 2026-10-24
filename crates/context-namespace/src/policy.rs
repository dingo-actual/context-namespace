use crate::config::NamespaceConfig;
use crate::error::NamespaceError;

/// Validates cross-field constraints on a [`NamespaceConfig`].
///
/// # Errors
///
/// Returns [`NamespaceError::Config`] if any constraint is violated.
pub fn validate_config(config: &NamespaceConfig) -> Result<(), NamespaceError> {
    let policy = &config.policy;

    if config.render_field.is_empty() {
        return Err(NamespaceError::Config(
            "render_field must not be empty".to_string(),
        ));
    }

    if policy.trigger_tokens < policy.target_tokens {
        return Err(NamespaceError::Config(format!(
            "trigger_tokens ({}) must be >= target_tokens ({})",
            policy.trigger_tokens, policy.target_tokens
        )));
    }

    if policy.effective_budget_tokens < policy.trigger_tokens {
        return Err(NamespaceError::Config(format!(
            "effective_budget_tokens ({}) must be >= trigger_tokens ({})",
            policy.effective_budget_tokens, policy.trigger_tokens
        )));
    }

    if policy.preserve_prefix_records == 0 {
        return Err(NamespaceError::Config(
            "preserve_prefix_records must be > 0".to_string(),
        ));
    }

    if policy.preserve_recent_records == 0 {
        return Err(NamespaceError::Config(
            "preserve_recent_records must be > 0".to_string(),
        ));
    }

    if policy.default_expansion_ttl.get() > policy.max_expansion_ttl.get() {
        return Err(NamespaceError::Config(format!(
            "default_expansion_ttl ({}) must be <= max_expansion_ttl ({})",
            policy.default_expansion_ttl, policy.max_expansion_ttl
        )));
    }

    Ok(())
}

impl NamespaceConfig {
    /// Validates cross-field constraints on this configuration.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Config`] if any constraint is violated.
    pub fn validate(&self) -> Result<(), NamespaceError> {
        validate_config(self)
    }
}
