use chrono::{DateTime, Local};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    NeedsAction,
    InProcess,
    Completed,
    Cancelled,
}

impl Status {
    pub fn as_ical(self) -> &'static str {
        match self {
            Self::NeedsAction => "NEEDS-ACTION",
            Self::InProcess => "IN-PROCESS",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "IN-PROCESS" => Self::InProcess,
            "COMPLETED" => Self::Completed,
            "CANCELLED" => Self::Cancelled,
            _ => Self::NeedsAction,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.as_ical()
    }

    pub fn parse_filter(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "NEEDS-ACTION" => Some(Self::NeedsAction),
            "IN-PROCESS" => Some(Self::InProcess),
            "COMPLETED" => Some(Self::Completed),
            "CANCELLED" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Todo {
    pub uid: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub due: Option<DateTime<Local>>,
    pub start: Option<DateTime<Local>>,
    pub status: Status,
    pub priority: Option<u8>,
    pub categories: Vec<String>,
    pub percent_complete: u8,
    pub list_name: String,
    pub path: PathBuf,
    pub raw_other: Vec<String>,
}

impl Todo {
    pub fn done_marker(&self) -> &'static str {
        if self.status == Status::Completed { "[X]" } else { "[ ]" }
    }

    pub fn priority_marker(&self) -> &'static str {
        match self.priority.unwrap_or(0) {
            1..=3 => "!!!",
            4..=6 => "!!",
            7..=9 => "!",
            _ => "",
        }
    }
}
