//! The one-line status strip and the current error, shared by the shell and the core.
//!
//! A status is either a locale key or literal text; the shell translates keys once at the
//! point it draws the strip, so both kinds live in the same string.

/// A short message above the conversation, such as the reason a send was refused.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Status(pub Box<str>);

impl Status {
	pub const EMPTY: &'static str = "";
	pub fn new(text: impl Into<Box<str>>) -> Self {
		Self(text.into())
	}
	pub fn as_str(&self) -> &str {
		&self.0
	}
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
	pub fn set(&mut self, text: impl Into<Box<str>>) {
		self.0 = text.into();
	}
	pub fn clear(&mut self) {
		self.0 = Box::default();
	}
}

/// An error value converts to the message it shows.
impl From<crate::auth::Failure> for Status {
	fn from(failure: crate::auth::Failure) -> Self {
		Self(failure.label().into())
	}
}
impl From<&'static str> for Status {
	fn from(text: &'static str) -> Self {
		Self(text.into())
	}
}
impl From<String> for Status {
	fn from(text: String) -> Self {
		Self(text.into_boxed_str())
	}
}
impl From<Box<str>> for Status {
	fn from(text: Box<str>) -> Self {
		Self(text)
	}
}
impl std::ops::Deref for Status {
	type Target = str;
	fn deref(&self) -> &str {
		&self.0
	}
}
impl std::fmt::Display for Status {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}
impl PartialEq<&str> for Status {
	fn eq(&self, other: &&str) -> bool {
		self.0.as_ref() == *other
	}
}
impl PartialEq<str> for Status {
	fn eq(&self, other: &str) -> bool {
		self.0.as_ref() == other
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn literals_keys_and_owned_text_share_one_type() {
		let mut status = Status::from("lib-status-paste-cancelled");
		assert_eq!(status, "lib-status-paste-cancelled");
		assert!(!status.is_empty());
		status.set(String::from("Session expired"));
		assert!(status.starts_with("Session"));
		status.clear();
		assert!(status.is_empty());
		assert_eq!(status.as_str(), Status::EMPTY);
	}
}
