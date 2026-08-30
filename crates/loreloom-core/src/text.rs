use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TextError {
    #[error("text exceeds its {maximum}-byte UTF-8 limit")]
    TooLong { maximum: usize },
    #[error("text cannot be empty")]
    Empty,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoundedText<const MAX: usize>(String);

impl<const MAX: usize> BoundedText<MAX> {
    pub fn new(value: impl Into<String>) -> Result<Self, TextError> {
        let value = value.into();
        if value.len() > MAX {
            return Err(TextError::TooLong { maximum: MAX });
        }
        Ok(Self(value))
    }

    pub fn non_empty(value: impl Into<String>) -> Result<Self, TextError> {
        let value = Self::new(value)?;
        if value.0.is_empty() {
            return Err(TextError::Empty);
        }
        Ok(value)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<const MAX: usize> fmt::Display for BoundedText<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<const MAX: usize> fmt::Debug for BoundedText<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<const MAX: usize> Serialize for BoundedText<MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for BoundedText<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

pub type DisplayName = BoundedText<256>;
pub type ShortText = BoundedText<4096>;
pub type LongText = BoundedText<65536>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_text_counts_utf8_bytes_and_revalidates_serde() {
        assert!(BoundedText::<4>::new("世界").is_err());
        let value = BoundedText::<6>::new("世界").expect("six UTF-8 bytes");
        let json = serde_json::to_string(&value).expect("serialize text");
        assert_eq!(
            serde_json::from_str::<BoundedText<6>>(&json).expect("deserialize bounded text"),
            value
        );
        assert!(serde_json::from_str::<BoundedText<5>>(&json).is_err());
    }
}
