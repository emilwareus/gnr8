//! RED placeholder — the real `CoreError` (with `NotYetImplemented`) lands in the GREEN step.
//! This intentionally lacks the variant the tests assert on so the suite fails to compile (RED).

/// Placeholder enum without variants — replaced in the GREEN step.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {}
