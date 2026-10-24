use std::num::NonZeroU32;

use crate::NodeLabel;
use crate::error::WorkflowError;
use crate::namespace::ContextNamespace;
use crate::summary::{
    MaintenanceDecision, MaintenanceInput, MaintenanceOutcome, PreparedSummary, RecordSelection,
    SummaryInput, SummaryResult, TextSelection,
};

/// Harness-supplied summarizer abstraction.
///
/// A Rust agent implements this trait with its preferred model SDK. The
/// library calls `summarize` between `prepare_*` and `commit_summary`,
/// so no mutable state is held during inference.
#[cfg(feature = "async-adapters")]
#[async_trait::async_trait]
pub trait Summarizer: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error>;
}

impl ContextNamespace {
    fn cancel_prepared(&mut self, prepared: &PreparedSummary) {
        let _ = self.cancel_summary(prepared.request_id);
    }

    /// High-level workflow: prepare a record-range summary, call the
    /// summarizer, and commit the result.
    ///
    /// If inference fails, the pending request is cancelled and the
    /// inference error is returned. Canonical state is not mutated.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Inference`] if the summarizer fails, or
    /// [`WorkflowError::Core`] if a namespace operation fails.
    #[cfg(feature = "async-adapters")]
    pub async fn summarize_records_with<S>(
        &mut self,
        selection: RecordSelection,
        label: NodeLabel,
        summarizer: &S,
    ) -> Result<crate::SummaryReceipt, WorkflowError<S::Error>>
    where
        S: Summarizer,
    {
        let prepared = self.prepare_record_summary(selection, label)?;
        let result = match summarizer.summarize(prepared.input.clone()).await {
            Ok(result) => result,
            Err(error) => {
                self.cancel_prepared(&prepared);
                return Err(WorkflowError::Inference(error));
            }
        };
        self.commit_summary(prepared, result)
            .map_err(WorkflowError::Core)
    }

    /// High-level workflow: prepare a text-slice summary, call the
    /// summarizer, and commit the result.
    ///
    /// If inference fails, the pending request is cancelled and the
    /// inference error is returned. Canonical state is not mutated.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Inference`] if the summarizer fails, or
    /// [`WorkflowError::Core`] if a namespace operation fails.
    #[cfg(feature = "async-adapters")]
    pub async fn summarize_text_with<S>(
        &mut self,
        selection: TextSelection,
        label: NodeLabel,
        summarizer: &S,
    ) -> Result<crate::SummaryReceipt, WorkflowError<S::Error>>
    where
        S: Summarizer,
    {
        let prepared = self.prepare_text_summary(selection, label)?;
        let result = match summarizer.summarize(prepared.input.clone()).await {
            Ok(result) => result,
            Err(error) => {
                self.cancel_prepared(&prepared);
                return Err(WorkflowError::Inference(error));
            }
        };
        self.commit_summary(prepared, result)
            .map_err(WorkflowError::Core)
    }

    /// High-level workflow: plan maintenance, optionally call the
    /// summarizer, and commit if triggered.
    ///
    /// If the policy decides no action is needed, returns
    /// [`MaintenanceOutcome::NoAction`] without calling the summarizer.
    /// If summarization is triggered and inference fails, the pending
    /// request is cancelled and the inference error is returned.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Inference`] if the summarizer fails, or
    /// [`WorkflowError::Core`] if a namespace operation fails.
    #[cfg(feature = "async-adapters")]
    pub async fn maintain_with<S>(
        &mut self,
        input: MaintenanceInput,
        summarizer: &S,
    ) -> Result<MaintenanceOutcome, WorkflowError<S::Error>>
    where
        S: Summarizer,
    {
        let decision = self.plan_maintenance(input)?;
        match decision {
            MaintenanceDecision::NoAction { stats } => Ok(MaintenanceOutcome::NoAction { stats }),
            MaintenanceDecision::Summarize(prepared) => {
                let result = match summarizer.summarize(prepared.input.clone()).await {
                    Ok(result) => result,
                    Err(error) => {
                        self.cancel_prepared(&prepared);
                        return Err(WorkflowError::Inference(error));
                    }
                };
                let receipt = self.commit_summary(prepared, result)?;
                Ok(MaintenanceOutcome::Summarized(receipt))
            }
        }
    }

    /// Expands a node by path or ID for `ttl` successful renders.
    ///
    /// Convenience wrapper around [`expand`](ContextNamespace::expand) that
    /// accepts a [`NodeLocator`](crate::NodeLocator) (path or node ID) and a
    /// type-safe non-zero TTL.
    ///
    /// # Errors
    ///
    /// Returns [`crate::NamespaceError`] if the locator cannot be resolved
    /// or limits are exceeded.
    #[cfg(feature = "async-adapters")]
    pub fn expand_node(
        &mut self,
        locator: crate::NodeLocator,
        ttl: NonZeroU32,
    ) -> Result<crate::ExpansionReceipt, crate::NamespaceError> {
        let path = match locator {
            crate::NodeLocator::Path(path) => path,
            crate::NodeLocator::Id(id) => self.current_path(id)?,
        };
        self.expand(&path, ttl.get())
    }
}
