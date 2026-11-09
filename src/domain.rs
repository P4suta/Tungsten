use std::fmt;

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use crate::rules::{revision_matches, valid_name};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct NoteName(String);

impl NoteName {
    pub fn parse(name: String) -> Result<Self, crate::Error> {
        if valid_name(name.as_bytes()) {
            Ok(Self(name))
        } else {
            Err(crate::Error::InvalidName)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for NoteName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(name).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Revision(String);

impl Revision {
    pub fn parse(revision: String) -> Result<Self, crate::Error> {
        if revision.len() != 66
            || !revision.starts_with('"')
            || !revision.ends_with('"')
            || !revision.as_bytes()[1..65].iter().all(u8::is_ascii_hexdigit)
        {
            return Err(crate::Error::InvalidData("The revision is invalid."));
        }
        Ok(Self(revision))
    }

    pub fn of(body: &[u8]) -> Self {
        Self::of_parts(&[body])
    }

    pub fn of_parts(parts: &[&[u8]]) -> Self {
        let mut digest = Sha256::new();
        for part in parts {
            digest.update(part);
        }
        let hash: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self(format!("\"{hash}\""))
    }

    pub fn matches(&self, expected: &str) -> bool {
        revision_matches(expected.as_bytes(), self.0.as_bytes())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for Revision {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let revision = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(revision).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
