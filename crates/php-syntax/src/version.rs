//! PHP language versions.

use std::fmt;
use std::str::FromStr;

/// A PHP language version (`major.minor`) used to gate syntax features.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PhpVersion {
    pub major: u8,
    pub minor: u8,
}

impl PhpVersion {
    pub const PHP_5_3: PhpVersion = PhpVersion::new(5, 3);
    pub const PHP_5_4: PhpVersion = PhpVersion::new(5, 4);
    pub const PHP_5_5: PhpVersion = PhpVersion::new(5, 5);
    pub const PHP_5_6: PhpVersion = PhpVersion::new(5, 6);
    pub const PHP_7_0: PhpVersion = PhpVersion::new(7, 0);
    pub const PHP_7_1: PhpVersion = PhpVersion::new(7, 1);
    pub const PHP_7_2: PhpVersion = PhpVersion::new(7, 2);
    pub const PHP_7_3: PhpVersion = PhpVersion::new(7, 3);
    pub const PHP_7_4: PhpVersion = PhpVersion::new(7, 4);
    pub const PHP_8_0: PhpVersion = PhpVersion::new(8, 0);
    pub const PHP_8_1: PhpVersion = PhpVersion::new(8, 1);
    pub const PHP_8_2: PhpVersion = PhpVersion::new(8, 2);
    pub const PHP_8_3: PhpVersion = PhpVersion::new(8, 3);
    pub const PHP_8_4: PhpVersion = PhpVersion::new(8, 4);
    pub const PHP_8_5: PhpVersion = PhpVersion::new(8, 5);

    /// The newest version this parser knows about.
    pub const LATEST: PhpVersion = PhpVersion::PHP_8_5;

    pub const fn new(major: u8, minor: u8) -> PhpVersion {
        PhpVersion { major, minor }
    }
}

impl Default for PhpVersion {
    fn default() -> Self {
        PhpVersion::PHP_8_4
    }
}

impl fmt::Display for PhpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for PhpVersion {
    type Err = String;

    /// Parses `8`, `8.1` or `8.1.12` (the patch level is ignored).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.trim().split('.');
        let major = parts
            .next()
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| format!("invalid PHP version `{s}`"))?;
        let minor = match parts.next() {
            Some(p) => p.parse::<u8>().map_err(|_| format!("invalid PHP version `{s}`"))?,
            None => 0,
        };
        Ok(PhpVersion::new(major, minor))
    }
}
