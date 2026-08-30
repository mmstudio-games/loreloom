use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FixedError {
    #[error("fixed-point arithmetic overflow")]
    Overflow,
    #[error("fixed-point division by zero")]
    DivisionByZero,
    #[error("world time overflow")]
    WorldTimeOverflow,
}

/// A deterministic signed fixed-point value serialized as raw millionths.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Fixed(i64);

impl Fixed {
    pub const SCALE: i64 = 1_000_000;
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(Self::SCALE);

    #[must_use]
    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub fn from_integer(value: i64) -> Result<Self, FixedError> {
        value
            .checked_mul(Self::SCALE)
            .map(Self)
            .ok_or(FixedError::Overflow)
    }

    #[must_use]
    pub const fn micros(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, FixedError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(FixedError::Overflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, FixedError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(FixedError::Overflow)
    }

    pub fn checked_mul(self, other: Self) -> Result<Self, FixedError> {
        round_ratio(
            i128::from(self.0) * i128::from(other.0),
            i128::from(Self::SCALE),
        )
        .and_then(i64_from_i128)
        .map(Self)
    }

    pub fn checked_div(self, other: Self) -> Result<Self, FixedError> {
        if other.0 == 0 {
            return Err(FixedError::DivisionByZero);
        }
        round_ratio(
            i128::from(self.0) * i128::from(Self::SCALE),
            i128::from(other.0),
        )
        .and_then(i64_from_i128)
        .map(Self)
    }

    #[must_use]
    pub fn clamp(self, minimum: Self, maximum: Self) -> Self {
        Self(self.0.clamp(minimum.0, maximum.0))
    }
}

fn i64_from_i128(value: i128) -> Result<i64, FixedError> {
    i64::try_from(value).map_err(|_| FixedError::Overflow)
}

fn round_ratio(numerator: i128, denominator: i128) -> Result<i128, FixedError> {
    if denominator == 0 {
        return Err(FixedError::DivisionByZero);
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let twice_remainder = remainder.abs().checked_mul(2).ok_or(FixedError::Overflow)?;
    let denominator_abs = denominator.abs();
    let round_away = twice_remainder > denominator_abs
        || (twice_remainder == denominator_abs && quotient % 2 != 0);
    if !round_away {
        return Ok(quotient);
    }
    let adjustment = if numerator.signum() == denominator.signum() {
        1
    } else {
        -1
    };
    quotient.checked_add(adjustment).ok_or(FixedError::Overflow)
}

impl fmt::Display for Fixed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let negative = self.0.is_negative();
        let absolute = i128::from(self.0).abs();
        let whole = absolute / i128::from(Self::SCALE);
        let fraction = absolute % i128::from(Self::SCALE);
        if negative {
            formatter.write_str("-")?;
        }
        write!(formatter, "{whole}.{fraction:06}")
    }
}

/// Logical in-world seconds elapsed from world creation.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct WorldTime(u64);

impl WorldTime {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn from_ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    #[must_use]
    pub const fn ticks(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, ticks: u64) -> Result<Self, FixedError> {
        self.0
            .checked_add(ticks)
            .map(Self)
            .ok_or(FixedError::WorldTimeOverflow)
    }
}

impl fmt::Display for WorldTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_uses_integer_wire_and_checked_arithmetic() {
        let one_and_half = Fixed::from_micros(1_500_000);
        assert_eq!(
            serde_json::to_string(&one_and_half).expect("serialize"),
            "1500000"
        );
        assert_eq!(
            one_and_half.checked_mul(Fixed::from_integer(2).expect("two")),
            Ok(Fixed::from_integer(3).expect("three"))
        );
        assert_eq!(
            Fixed::from_micros(i64::MAX).checked_add(Fixed::ONE),
            Err(FixedError::Overflow)
        );
        assert_eq!(
            Fixed::ONE.checked_div(Fixed::ZERO),
            Err(FixedError::DivisionByZero)
        );
    }

    #[test]
    fn fixed_rounds_half_to_even_for_both_signs() {
        let half_micro_factor = Fixed::from_micros(500_000);
        assert_eq!(
            Fixed::from_micros(1).checked_mul(half_micro_factor),
            Ok(Fixed::ZERO)
        );
        assert_eq!(
            Fixed::from_micros(3).checked_mul(half_micro_factor),
            Ok(Fixed::from_micros(2))
        );
        assert_eq!(
            Fixed::from_micros(-3).checked_mul(half_micro_factor),
            Ok(Fixed::from_micros(-2))
        );
    }
}
