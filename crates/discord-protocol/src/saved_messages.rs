//! Saved messages: community-documented user routes, not verified against a live session.
//! GET uses `{results: [...]}`; PUT returns one SavedMessage. Summaries and notes are not writes.

use crate::{DecodeError, Timestamp, lossy::Slots};
use model::{
	Id,
	saved_messages::{self as saved, MessagePreview, SavedMessage},
};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

pub const MAX_SAVED_MESSAGES_WIRE: usize = 4 * 1024 * 1024;

fn text<'de, D: Deserializer<'de>, const N: usize>(d: D) -> Result<String, D::Error> {
	struct Text<const N: usize>;
	impl<const N: usize> serde::de::Visitor<'_> for Text<N> {
		type Value = String;
		fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
			f.write_str("a bounded string")
		}
		fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
			if value.len() > N {
				return Err(E::custom("Saved-message text capacity"));
			}
			Ok(value.to_owned())
		}
	}
	d.deserialize_str(Text::<N>)
}

#[derive(Deserialize)]
struct AuthorDto {
	id: Id,
	#[serde(deserialize_with = "text::<_, { saved::MAX_AUTHOR_BYTES }>")]
	username: String,
	#[serde(default)]
	global_name: Option<AuthorName>,
}

#[derive(Deserialize)]
struct AuthorName(#[serde(deserialize_with = "text::<_, { saved::MAX_AUTHOR_BYTES }>")] String);

#[derive(Deserialize)]
struct MessageDto {
	id: Id,
	channel_id: Id,
	#[serde(default)]
	guild_id: Option<Id>,
	author: AuthorDto,
	#[serde(deserialize_with = "text::<_, { saved::MAX_CONTENT_BYTES }>")]
	content: String,
}

/// The short string bound precedes RFC 3339 parsing, including escaped JSON strings.
#[derive(Deserialize)]
struct ServiceTime(#[serde(deserialize_with = "text::<_, 64>")] String);
impl ServiceTime {
	fn seconds(self) -> Result<i64, DecodeError> {
		Timestamp::try_from(self.0)
			.map_err(|_| DecodeError)?
			.unix_ms()
			.map(|time| time.div_euclid(1000))
			.ok_or(DecodeError)
	}
}

#[derive(Deserialize)]
struct SaveDataDto {
	channel_id: Id,
	message_id: Id,
	#[serde(default)]
	guild_id: Option<Id>,
	saved_at: ServiceTime,
	#[serde(deserialize_with = "Option::deserialize")]
	due_at: Option<ServiceTime>,
	#[serde(deserialize_with = "text::<_, { saved::MAX_SUMMARY_BYTES }>")]
	author_summary: String,
	#[serde(deserialize_with = "text::<_, { saved::MAX_SUMMARY_BYTES }>")]
	channel_summary: String,
	#[serde(deserialize_with = "text::<_, { saved::MAX_SUMMARY_BYTES }>")]
	message_summary: String,
	#[serde(
		rename = "notes",
		deserialize_with = "text::<_, { saved::MAX_SUMMARY_BYTES }>"
	)]
	_notes: String,
}

#[derive(Deserialize)]
struct SavedMessageDto {
	// Absent is malformed; explicit null is a removable unavailable row.
	#[serde(deserialize_with = "Option::deserialize")]
	message: Option<MessageDto>,
	save_data: SaveDataDto,
}
impl SavedMessageDto {
	fn into_model(self) -> Result<SavedMessage, DecodeError> {
		let data = self.save_data;
		let preview = match self.message {
			Some(message) => {
				if message.id != data.message_id
					|| message.channel_id != data.channel_id
					|| message
						.guild_id
						.is_some_and(|guild| Some(guild) != data.guild_id)
					|| message.author.id.0 == 0
				{
					return Err(DecodeError);
				}
				Some(MessagePreview {
					author: message
						.author
						.global_name
						.map(|name| name.0)
						.filter(|name| !name.is_empty())
						.unwrap_or(message.author.username),
					content: message.content,
				})
			}
			None => None,
		};
		let row = SavedMessage {
			channel: data.channel_id,
			message: data.message_id,
			guild: data.guild_id,
			saved_at: data.saved_at.seconds()?,
			due_at: data.due_at.map(ServiceTime::seconds).transpose()?,
			author_summary: data.author_summary,
			channel_summary: data.channel_summary,
			message_summary: data.message_summary,
			preview,
		};
		row.validate().map_err(|_| DecodeError)?;
		Ok(row)
	}
}

#[derive(Deserialize)]
struct ListDto<'a> {
	#[serde(borrow)]
	results: &'a RawValue,
}

pub fn decode_list(bytes: &[u8]) -> Result<Vec<SavedMessage>, DecodeError> {
	if bytes.len() > MAX_SAVED_MESSAGES_WIRE {
		return Err(DecodeError);
	}
	let dto: ListDto<'_> = serde_json::from_slice(bytes).map_err(|_| DecodeError)?;
	// Slots deliberately accepts null in login snapshots; this endpoint requires an array.
	if !dto.results.get().starts_with('[') {
		return Err(DecodeError);
	}
	let slots: Slots<SavedMessageDto, { saved::MAX_SAVED_MESSAGES }> =
		serde_json::from_str(dto.results.get()).map_err(|_| DecodeError)?;
	let rows = slots
		.items
		.into_iter()
		.map(|row| row.ok_or(DecodeError)?.into_model())
		.collect::<Result<Vec<_>, _>>()?;
	saved::normalize_list(rows).map_err(|_| DecodeError)
}

pub fn decode_saved_message(bytes: &[u8]) -> Result<SavedMessage, DecodeError> {
	if bytes.len() > MAX_SAVED_MESSAGES_WIRE {
		return Err(DecodeError);
	}
	crate::decode::<SavedMessageDto>(bytes)?.into_model()
}

#[cfg(test)]
mod tests {
	use super::*;

	const ROW: &str = r#"{"message":null,"save_data":{"channel_id":"1","message_id":"2","saved_at":"2026-11-01T12:00:00Z","due_at":null,"author_summary":"opaque author","channel_summary":"opaque channel","message_summary":"not a content fallback","notes":"read only"}}"#;
	const MESSAGE: &str = r#"{"id":"2","channel_id":"1","author":{"id":"3","username":"Author"},"content":"Actual ||spoiler||"}"#;

	fn list(rows: &str) -> Vec<u8> {
		format!(r#"{{"results":[{rows}]}}"#).into_bytes()
	}

	#[test]
	fn wrapper_null_message_and_real_content_have_distinct_meanings() {
		let rows = decode_list(&list(ROW)).unwrap();
		assert_eq!(rows[0].key(), (Id(1), Id(2)));
		assert!(rows[0].preview.is_none());
		let raw = ROW.replacen("\"message\":null", &format!("\"message\":{MESSAGE}"), 1);
		let row = decode_saved_message(raw.as_bytes()).unwrap();
		assert_eq!(row.preview.unwrap().content, "Actual ||spoiler||");
		for raw in [
			format!("[{ROW}]"),
			"{}".into(),
			r#"{"results":null}"#.into(),
		] {
			assert!(decode_list(raw.as_bytes()).is_err());
		}
	}

	#[test]
	fn malformed_rows_missing_fields_and_identity_mismatches_reject_the_whole_list() {
		assert!(decode_list(&list(&format!("{ROW},{{}}"))).is_err());
		assert!(decode_saved_message(ROW.replace("\"message\":null,", "").as_bytes()).is_err());
		assert!(decode_saved_message(ROW.replace("\"due_at\":null,", "").as_bytes()).is_err());
		for message in [
			MESSAGE.replace("\"id\":\"2\"", "\"id\":\"9\""),
			MESSAGE.replace("\"channel_id\":\"1\"", "\"channel_id\":\"9\""),
			MESSAGE.replace(
				"Actual ||spoiler||",
				&"x".repeat(saved::MAX_CONTENT_BYTES + 1),
			),
		] {
			let raw = ROW.replacen("\"message\":null", &format!("\"message\":{message}"), 1);
			assert!(decode_list(&list(&raw)).is_err());
		}
		for raw in [
			ROW.replace("2026-11-01T12:00:00Z", "invalid"),
			ROW.replace("\"due_at\":null", "\"due_at\":\"invalid\""),
			ROW.replace("opaque author", &"x".repeat(saved::MAX_SUMMARY_BYTES + 1)),
			ROW.replace("read only", &"x".repeat(saved::MAX_SUMMARY_BYTES + 1)),
		] {
			assert!(decode_saved_message(raw.as_bytes()).is_err());
		}
	}

	#[test]
	fn duplicates_keep_first_but_never_bypass_raw_count_or_byte_caps() {
		let second = ROW.replace("opaque author", "later");
		let rows = decode_list(&list(&format!("{ROW},{second}"))).unwrap();
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].author_summary, "opaque author");
		let raw = std::iter::repeat_n(ROW, saved::MAX_SAVED_MESSAGES + 1)
			.collect::<Vec<_>>()
			.join(",");
		assert!(decode_list(&list(&raw)).is_err());
		let mut bytes = list(ROW);
		bytes.resize(MAX_SAVED_MESSAGES_WIRE + 1, b' ');
		assert!(decode_list(&bytes).is_err());
		assert!(decode_saved_message(&bytes).is_err());
	}
}
