//! Limits from docs/protocol.md and checks for values the client sends.

/// Max message body length, in Unicode code points.
pub const MAX_BODY_CHARS: usize = 32_768;
/// Max topic title length, in Unicode code points.
pub const MAX_TITLE_CHARS: usize = 200;
/// Max `limit` for `fetch` and `history`.
pub const MAX_PAGE_LIMIT: u32 = 500;
/// Max size of one WebSocket frame, in bytes.
pub const MAX_FRAME_BYTES: usize = 1_048_576;
/// Member display name length range, in code points.
pub const MIN_NAME_CHARS: usize = 1;
pub const MAX_NAME_CHARS: usize = 64;
/// Default topic title: first line of body, cut to this many code points.
pub const DEFAULT_TITLE_CHARS: usize = 80;
/// Max `clientMsgId` length, in code points.
pub const MAX_CLIENT_MSG_ID_CHARS: usize = 64;

/// A value that breaks a protocol limit. `field` is the camelCase wire name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: &'static str,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}

pub type Validation = Result<(), ValidationError>;

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

/// Checks that `value` has `min..=max` code points.
pub fn check_chars(field: &'static str, value: &str, min: usize, max: usize) -> Validation {
    let n = value.chars().count();
    if n < min {
        return fail(field, format!("at least {min} characters"));
    }
    if n > max {
        return fail(field, format!("at most {max} characters"));
    }
    Ok(())
}

/// Non-empty id.
pub fn check_id(field: &'static str, value: &str) -> Validation {
    if value.is_empty() {
        fail(field, "must not be empty")
    } else {
        Ok(())
    }
}

pub fn check_ids(field: &'static str, values: &[String]) -> Validation {
    values.iter().try_for_each(|v| check_id(field, v))
}

pub fn check_body(field: &'static str, value: &str) -> Validation {
    check_chars(field, value, 0, MAX_BODY_CHARS)
}

pub fn check_title(value: &str) -> Validation {
    check_chars("title", value, 1, MAX_TITLE_CHARS)
}

pub fn check_name(field: &'static str, value: &str) -> Validation {
    check_chars(field, value, MIN_NAME_CHARS, MAX_NAME_CHARS)
}

pub fn check_limit(value: u32) -> Validation {
    if (1..=MAX_PAGE_LIMIT).contains(&value) {
        Ok(())
    } else {
        fail("limit", format!("must be 1..={MAX_PAGE_LIMIT}"))
    }
}

/// The default title of a new topic: the first line of `body`, cut to
/// [`DEFAULT_TITLE_CHARS`] code points.
pub fn default_title(body: &str) -> String {
    body.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(DEFAULT_TITLE_CHARS)
        .collect()
}
