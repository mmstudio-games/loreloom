use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RevisionError {
    #[error("revision overflow")]
    Overflow,
    #[error("expected revision {expected}, observed {observed}")]
    Conflict {
        expected: Revision,
        observed: Revision,
    },
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, RevisionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(RevisionError::Overflow)
    }

    pub fn ensure(self, observed: Self) -> Result<(), RevisionError> {
        if self == observed {
            Ok(())
        } else {
            Err(RevisionError::Conflict {
                expected: self,
                observed,
            })
        }
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_starts_at_zero_and_checks_overflow() {
        assert_eq!(Revision::default(), Revision::ZERO);
        assert_eq!(
            Revision::ZERO.next().expect("next revision"),
            Revision::new(1)
        );
        assert_eq!(Revision::new(u64::MAX).next(), Err(RevisionError::Overflow));
    }
}
