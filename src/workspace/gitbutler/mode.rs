// GitButler mode parsing and display.
// Uses anyhow for invalid configuration values.

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Off,
    Auto,
    Always,
}

impl Mode {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            other => bail!("unknown gitbutler mode '{other}'"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Always => "always",
        }
    }
}
