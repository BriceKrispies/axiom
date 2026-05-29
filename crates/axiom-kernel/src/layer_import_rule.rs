//! The rule deciding whether one layer may import another.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// The stateless rule governing layer imports.
///
/// The architecture invariant is simple and total: a layer at index `importer`
/// may import a layer at index `target` **iff** `target < importer`. That single
/// rule yields every property the architecture requires:
/// - a layer cannot import itself (`target == importer`),
/// - a layer cannot import a future/higher layer (`target > importer`),
/// - the kernel (index `0`) can import nothing, because no index is `< 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerImportRule;

impl LayerImportRule {
    /// Validate that the layer at `importer` may import the layer at `target`.
    ///
    /// Returns [`KernelErrorCode::SelfImport`] when the indices are equal, and
    /// [`KernelErrorCode::ForwardImport`] when `target` is higher than
    /// `importer`.
    pub const fn validate(importer: u16, target: u16) -> KernelResult<()> {
        if target == importer {
            return Err(KernelError::new(
                KernelErrorScope::Layer,
                KernelErrorCode::SelfImport,
                "a layer may not import itself",
            ));
        }
        if target > importer {
            return Err(KernelError::new(
                KernelErrorScope::Layer,
                KernelErrorCode::ForwardImport,
                "a layer may only import strictly lower (earlier) layers",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importing_an_earlier_layer_is_allowed() {
        assert!(LayerImportRule::validate(3, 0).is_ok());
        assert!(LayerImportRule::validate(3, 2).is_ok());
    }

    #[test]
    fn self_import_is_rejected() {
        let err = LayerImportRule::validate(2, 2).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::SelfImport);
    }

    #[test]
    fn forward_import_is_rejected() {
        let err = LayerImportRule::validate(1, 5).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::ForwardImport);
    }

    #[test]
    fn kernel_can_import_nothing() {
        // Index 0 against any target is either self (0) or forward (>0).
        assert_eq!(
            LayerImportRule::validate(0, 0).unwrap_err().code(),
            KernelErrorCode::SelfImport
        );
        assert_eq!(
            LayerImportRule::validate(0, 1).unwrap_err().code(),
            KernelErrorCode::ForwardImport
        );
    }
}
