use std::{cmp::Ordering, fmt, str::FromStr};

use serde::Deserialize;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl RuntimeVersion {
    pub const ZERO: Self = Self {
        major: 0,
        minor: 0,
        patch: 0,
    };

    pub fn is_zero(self) -> bool {
        self == Self::ZERO
    }
}

impl fmt::Display for RuntimeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for RuntimeVersion {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim().trim_start_matches('v');
        let mut parts = value.split('.');
        let major = parts
            .next()
            .unwrap_or("0")
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid major version `{value}`: {error}"))?;
        let minor = parts
            .next()
            .unwrap_or("0")
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid minor version `{value}`: {error}"))?;
        let patch = parts
            .next()
            .unwrap_or("0")
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid patch version `{value}`: {error}"))?;

        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl<'de> Deserialize<'de> for RuntimeVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl PartialOrd for RuntimeVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RuntimeVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeVersion;

    #[test]
    fn parses_short_versions() {
        assert_eq!(
            "24".parse::<RuntimeVersion>().unwrap(),
            RuntimeVersion {
                major: 24,
                minor: 0,
                patch: 0
            }
        );
        assert_eq!(
            "v20.5.1".parse::<RuntimeVersion>().unwrap().to_string(),
            "20.5.1"
        );
    }
}
