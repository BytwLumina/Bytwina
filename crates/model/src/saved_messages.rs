//! Session-only saved-message projections. Service summaries are opaque, read-only strings.

use crate::Id;
use std::collections::BTreeSet;

pub const MAX_BOOKMARKS: usize = 200;
/// Includes bookmarks and reminders; independent of the bookmark ceiling.
pub const MAX_SAVED_MESSAGES: usize = 500;
pub const MAX_SAVED_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_SUMMARY_BYTES: usize = 4096;
pub const MAX_CONTENT_BYTES: usize = 16 * 1024;
pub const MAX_AUTHOR_BYTES: usize = 256;
pub const MAX_QUERY_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessagePreview {
	pub author: String,
	/// Actual message content, not `save_data.message_summary`. Render with spoiler handling.
	pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedMessage {
	pub channel: Id,
	pub message: Id,
	pub guild: Option<Id>,
	/// All times are Unix seconds, not milliseconds.
	pub saved_at: i64,
	pub due_at: Option<i64>,
	pub author_summary: String,
	pub channel_summary: String,
	pub message_summary: String,
	/// `None` means unavailable, not proof that the original was deleted.
	pub preview: Option<MessagePreview>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
	Malformed,
	Capacity,
}

/// RFC 3339 dates representable by this feature (through year 9999).
pub fn valid_timestamp(seconds: i64) -> bool {
	(0..=253_402_300_799).contains(&seconds)
}

impl SavedMessage {
	pub fn key(&self) -> (Id, Id) {
		(self.channel, self.message)
	}

	pub fn heap_bytes(&self) -> usize {
		self.author_summary
			.capacity()
			.saturating_add(self.channel_summary.capacity())
			.saturating_add(self.message_summary.capacity())
			.saturating_add(self.preview.as_ref().map_or(0, |preview| {
				preview
					.author
					.capacity()
					.saturating_add(preview.content.capacity())
			}))
	}

	pub fn validate(&self) -> Result<(), ValidationError> {
		if self.channel.0 == 0
			|| self.message.0 == 0
			|| self.guild.is_some_and(|id| id.0 == 0)
			|| !valid_timestamp(self.saved_at)
			|| self.due_at.is_some_and(|time| !valid_timestamp(time))
		{
			return Err(ValidationError::Malformed);
		}
		if [
			&self.author_summary,
			&self.channel_summary,
			&self.message_summary,
		]
		.iter()
		.any(|text| text.capacity() > MAX_SUMMARY_BYTES)
			|| self.preview.as_ref().is_some_and(|preview| {
				preview.author.capacity() > MAX_AUTHOR_BYTES
					|| preview.content.capacity() > MAX_CONTENT_BYTES
			}) {
			return Err(ValidationError::Capacity);
		}
		Ok(())
	}
}

/// Validates the entire input before deduplicating. First occurrence wins, in service order;
/// malformed duplicates cannot hide behind a valid row, and failures never return a partial list.
pub fn normalize_list(mut rows: Vec<SavedMessage>) -> Result<Vec<SavedMessage>, ValidationError> {
	if rows.len() > MAX_SAVED_MESSAGES {
		return Err(ValidationError::Capacity);
	}
	let mut bytes = rows.capacity().saturating_mul(size_of::<SavedMessage>());
	for row in &rows {
		row.validate()?;
		bytes = bytes.saturating_add(row.heap_bytes());
		if bytes > MAX_SAVED_MESSAGE_BYTES {
			return Err(ValidationError::Capacity);
		}
	}
	if bytes > MAX_SAVED_MESSAGE_BYTES {
		return Err(ValidationError::Capacity);
	}
	let mut seen = BTreeSet::new();
	rows.retain(|row| seen.insert(row.key()));
	if rows.iter().filter(|row| row.due_at.is_none()).count() > MAX_BOOKMARKS {
		return Err(ValidationError::Capacity);
	}
	rows.shrink_to_fit();
	Ok(rows)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn row(id: u64, due_at: Option<i64>) -> SavedMessage {
		SavedMessage {
			channel: Id(1),
			message: Id(id),
			guild: None,
			saved_at: 1,
			due_at,
			author_summary: String::new(),
			channel_summary: String::new(),
			message_summary: String::new(),
			preview: None,
		}
	}

	#[test]
	fn bookmark_and_total_caps_are_independent_and_duplicates_are_stable() {
		let mut rows: Vec<_> = (1..=200).map(|id| row(id, None)).collect();
		rows.extend((201..=500).map(|id| row(id, Some(2))));
		assert_eq!(normalize_list(rows.clone()).unwrap().len(), 500);
		rows.push(row(501, Some(2)));
		assert_eq!(normalize_list(rows), Err(ValidationError::Capacity));
		assert_eq!(
			normalize_list((1..=201).map(|id| row(id, None)).collect()),
			Err(ValidationError::Capacity)
		);
		let first = row(1, None);
		assert_eq!(
			normalize_list(vec![first.clone(), row(1, Some(2)), row(2, None)]).unwrap(),
			vec![first, row(2, None)]
		);
	}

	#[test]
	fn aggregate_bytes_and_hidden_allocation_are_bounded() {
		let mut rows: Vec<_> = (1..=100).map(|id| row(id, Some(2))).collect();
		for row in &mut rows {
			row.preview = Some(MessagePreview {
				author: "Author".into(),
				content: "x".repeat(MAX_CONTENT_BYTES),
			});
		}
		assert_eq!(normalize_list(rows), Err(ValidationError::Capacity));
		let mut oversized = row(1, None);
		oversized.message_summary = String::with_capacity(MAX_SUMMARY_BYTES + 1);
		assert_eq!(oversized.validate(), Err(ValidationError::Capacity));
	}
}
