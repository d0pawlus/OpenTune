// SPDX-License-Identifier: GPL-3.0-or-later

/// Scalar storage types defined by the public MLG v1 specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    I64,
    F32,
}

impl FieldType {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::U8 => 0,
            Self::I8 => 1,
            Self::U16 => 2,
            Self::I16 => 3,
            Self::U32 => 4,
            Self::I32 => 5,
            Self::I64 => 6,
            Self::F32 => 7,
        }
    }

    pub(crate) fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::U8,
            1 => Self::I8,
            2 => Self::U16,
            3 => Self::I16,
            4 => Self::U32,
            5 => Self::I32,
            6 => Self::I64,
            7 => Self::F32,
            _ => return None,
        })
    }

    pub(crate) fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::I64 => 8,
        }
    }
}

/// A logger field. Physical value is `(raw + transform) * scale` in MLG v1.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub units: String,
    pub field_type: FieldType,
    pub display_style: u8,
    pub scale: f32,
    pub transform: f32,
    pub digits: u8,
}

impl Field {
    /// A loss-minimising field for decoded realtime values.
    pub fn float(name: impl Into<String>, units: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            units: units.into(),
            field_type: FieldType::F32,
            display_style: 0,
            scale: 1.0,
            transform: 0.0,
            digits: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub counter: u8,
    /// Monotonic timestamp in 10 µs units. MLG I/O unwraps/wraps its 16-bit
    /// wire representation at this model boundary.
    pub timestamp_10us: u64,
    /// Physical values in field order. Missing values are represented by NaN.
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub counter: u8,
    pub timestamp_10us: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogEntry {
    Record(Record),
    Marker(Marker),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Log {
    pub fields: Vec<Field>,
    /// Optional Unix timestamp from the MLG header. Zero means unavailable.
    pub started_unix: u32,
    pub entries: Vec<LogEntry>,
}

impl Log {
    pub fn new(fields: Vec<Field>) -> Self {
        Self {
            fields,
            started_unix: 0,
            entries: Vec::new(),
        }
    }

    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.entries.iter().filter_map(|entry| match entry {
            LogEntry::Record(record) => Some(record),
            LogEntry::Marker(_) => None,
        })
    }

    pub fn markers(&self) -> impl Iterator<Item = &Marker> {
        self.entries.iter().filter_map(|entry| match entry {
            LogEntry::Marker(marker) => Some(marker),
            LogEntry::Record(_) => None,
        })
    }
}
