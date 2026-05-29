//! The deterministic outcome of one system's `run` during one step.

use axiom_kernel::HandleId;

use crate::runtime_result::RuntimeResult;

/// What happened when the scheduler ran one system during a step.
///
/// Carries the kernel-typed stable `id` of the system (a [`HandleId`]), its
/// static name, the order value it was registered with, and the
/// `RuntimeResult` it returned. Outcomes appear in deterministic execution
/// order inside [`crate::runtime_diagnostics::RuntimeDiagnostics`].
#[derive(Debug, Clone)]
pub struct SystemOutcome {
    id: HandleId,
    name: &'static str,
    order: i32,
    result: RuntimeResult<()>,
}

impl SystemOutcome {
    pub fn new(id: HandleId, name: &'static str, order: i32, result: RuntimeResult<()>) -> Self {
        SystemOutcome {
            id,
            name,
            order,
            result,
        }
    }

    pub fn id(&self) -> HandleId {
        self.id
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn order(&self) -> i32 {
        self.order
    }

    pub fn result(&self) -> &RuntimeResult<()> {
        &self.result
    }

    pub fn succeeded(&self) -> bool {
        self.result.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error::RuntimeError;
    use crate::runtime_error_code::RuntimeErrorCode;

    #[test]
    fn accessors_return_constructed_parts() {
        let o = SystemOutcome::new(HandleId::from_raw(7), "physics", 10, Ok(()));
        assert_eq!(o.id(), HandleId::from_raw(7));
        assert_eq!(o.name(), "physics");
        assert_eq!(o.order(), 10);
        assert!(o.succeeded());
    }

    #[test]
    fn failed_outcome_records_error() {
        let o = SystemOutcome::new(
            HandleId::from_raw(1),
            "boom",
            0,
            Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x")),
        );
        assert!(!o.succeeded());
        assert_eq!(
            o.result().as_ref().unwrap_err().code(),
            RuntimeErrorCode::SystemFailed
        );
    }
}
