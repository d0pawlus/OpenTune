// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Debug)]
pub enum DatalogError {
    Io(std::io::Error),
    InvalidMagic([u8; 6]),
    UnsupportedVersion(u16),
    UnsupportedFieldType(u8),
    UnsupportedBlockType(u8),
    Truncated(&'static str),
    InvalidHeader(String),
    InvalidField { index: usize, reason: String },
    InvalidRecord { index: usize, reason: String },
    InvalidCsv { line: usize, reason: String },
    ValueOutOfRange { field: String, value: f64 },
}

pub type Result<T> = std::result::Result<T, DatalogError>;

impl std::fmt::Display for DatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "datalog I/O error: {error}"),
            Self::InvalidMagic(magic) => write!(f, "invalid MLG magic: {magic:?}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported MLG version {version}; only v1 is supported")
            }
            Self::UnsupportedFieldType(kind) => write!(f, "unsupported MLG field type {kind}"),
            Self::UnsupportedBlockType(kind) => write!(f, "unsupported MLG block type {kind}"),
            Self::Truncated(section) => write!(f, "truncated MLG {section}"),
            Self::InvalidHeader(reason) => write!(f, "invalid MLG header: {reason}"),
            Self::InvalidField { index, reason } => {
                write!(f, "invalid field {index}: {reason}")
            }
            Self::InvalidRecord { index, reason } => {
                write!(f, "invalid record {index}: {reason}")
            }
            Self::InvalidCsv { line, reason } => write!(f, "invalid CSV at line {line}: {reason}"),
            Self::ValueOutOfRange { field, value } => {
                write!(f, "value {value} is out of range for field `{field}`")
            }
        }
    }
}

impl std::error::Error for DatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DatalogError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
