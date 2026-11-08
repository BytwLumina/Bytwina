//! Soundboard catalog decoding. Unofficial normal-user routes; see docs/discord-compatibility.md.
use crate::DecodeError;
use model::{Id, SoundboardSound};
use serde::Deserialize;

/// Catalog response ceiling for the default and per-guild sound lists.
pub const MAX_SOUNDBOARD_WIRE: usize = 512 * 1024;

#[derive(Deserialize)]
struct SoundDto {
	sound_id: Id,
	name: String,
	#[serde(default)]
	volume: Option<f32>,
	#[serde(default)]
	emoji_id: Option<Id>,
	#[serde(default)]
	emoji_name: Option<String>,
	#[serde(default)]
	guild_id: Option<Id>,
	#[serde(default)]
	available: Option<bool>,
}

#[derive(Deserialize)]
struct GuildList {
	items: Vec<SoundDto>,
}

fn convert(dto: SoundDto, guild: Option<Id>) -> Option<SoundboardSound> {
	let sound = SoundboardSound {
		id: dto.sound_id,
		name: dto.name,
		volume: dto.volume.unwrap_or(1.0).clamp(0.0, 1.0),
		emoji_id: dto.emoji_id,
		emoji_name: dto.emoji_name,
		guild: dto.guild_id.or(guild),
		available: dto.available.unwrap_or(true),
	};
	sound.valid().then_some(sound)
}

fn collect(rows: Vec<SoundDto>, guild: Option<Id>) -> Vec<SoundboardSound> {
	let mut sounds = Vec::new();
	let mut seen = std::collections::BTreeSet::new();
	for row in rows {
		let Some(sound) = convert(row, guild) else {
			continue;
		};
		if !seen.insert(sound.id) {
			continue;
		}
		sounds.push(sound);
		if sounds.len() >= model::MAX_SOUNDBOARD_SOUNDS {
			break;
		}
	}
	sounds
}

/// `GET /soundboard-default-sounds` answers with a bare array.
pub fn decode_list(bytes: &[u8], guild: Option<Id>) -> Result<Vec<SoundboardSound>, DecodeError> {
	if bytes.len() > MAX_SOUNDBOARD_WIRE {
		return Err(DecodeError);
	}
	let rows: Vec<SoundDto> = crate::decode(bytes)?;
	Ok(collect(rows, guild))
}

/// `GET /guilds/{id}/soundboard-sounds` answers with `{ "items": [...] }`.
pub fn decode_guild_list(bytes: &[u8], guild: Id) -> Result<Vec<SoundboardSound>, DecodeError> {
	if bytes.len() > MAX_SOUNDBOARD_WIRE {
		return Err(DecodeError);
	}
	let list: GuildList = crate::decode(bytes)?;
	Ok(collect(list.items, Some(guild)))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn malformed_rows_are_dropped_and_duplicates_collapse() {
		let json = r#"[
			{"sound_id":"1","name":"Hello","volume":0.5,"emoji_id":null,"emoji_name":"wave","available":true},
			{"sound_id":"1","name":"Duplicate"},
			{"sound_id":"2","name":""},
			{"sound_id":"3","name":"Loud","volume":4.0},
			{"sound_id":"0","name":"Zero"},
			{"sound_id":"4","name":"Ok","guild_id":"77"}
		]"#;
		let sounds = decode_list(json.as_bytes(), None).unwrap();
		assert_eq!(sounds.len(), 3);
		assert_eq!(sounds[0].id, Id(1));
		assert_eq!(sounds[0].volume, 0.5);
		assert_eq!(sounds[1].volume, 1.0);
		assert_eq!(sounds[2].guild, Some(Id(77)));
	}

	#[test]
	fn guild_list_reads_the_items_wrapper() {
		let json = r#"{"items":[{"sound_id":"9","name":"Guild sound","available":false}]}"#;
		let sounds = decode_guild_list(json.as_bytes(), Id(5)).unwrap();
		assert_eq!(sounds.len(), 1);
		assert_eq!(sounds[0].guild, Some(Id(5)));
		assert!(!sounds[0].available);
	}

	#[test]
	fn oversized_answers_are_rejected() {
		let bytes = vec![b' '; MAX_SOUNDBOARD_WIRE + 1];
		assert!(decode_list(&bytes, None).is_err());
		assert!(decode_guild_list(&bytes, Id(1)).is_err());
	}
}
