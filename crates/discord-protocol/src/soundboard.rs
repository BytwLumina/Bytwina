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
#[serde(untagged)]
enum LenientRow {
	Row(SoundDto),
	Invalid(serde::de::IgnoredAny),
}
impl LenientRow {
	fn into_value(self) -> Option<SoundDto> {
		match self {
			Self::Row(row) => Some(row),
			Self::Invalid(_) => None,
		}
	}
}

#[derive(Deserialize)]
struct LenientGuildList {
	#[serde(default)]
	items: Vec<LenientRow>,
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
	let rows: Vec<LenientRow> = crate::decode(bytes)?;
	Ok(collect(
		rows.into_iter()
			.filter_map(LenientRow::into_value)
			.collect(),
		guild,
	))
}

/// `GET /guilds/{id}/soundboard-sounds` answers with `{ "items": [...] }`.
pub fn decode_guild_list(bytes: &[u8], guild: Id) -> Result<Vec<SoundboardSound>, DecodeError> {
	if bytes.len() > MAX_SOUNDBOARD_WIRE {
		return Err(DecodeError);
	}
	let list: LenientGuildList = crate::decode(bytes)?;
	Ok(collect(
		list.items
			.into_iter()
			.filter_map(LenientRow::into_value)
			.collect(),
		Some(guild),
	))
}

/// One soundboard effect broadcast to everyone in the voice channel.
pub use model::VoiceEffect;

#[derive(Deserialize)]
struct EffectDto {
	channel_id: Id,
	user_id: Id,
	sound_id: Id,
	#[serde(default)]
	sound_volume: Option<f32>,
	#[serde(default)]
	emoji: Option<EffectEmoji>,
}

#[derive(Deserialize)]
struct EffectEmoji {
	#[serde(default)]
	id: Option<Id>,
	#[serde(default)]
	name: Option<String>,
}

/// `VOICE_CHANNEL_EFFECT_SEND`. Emoji-only reactions carry no `sound_id` and are ignored here;
/// a malformed effect must never stop the connection.
pub fn voice_effect(bytes: &[u8]) -> Result<Option<VoiceEffect>, DecodeError> {
	if bytes.len() > MAX_SOUNDBOARD_WIRE {
		return Err(DecodeError);
	}
	let Ok(dto) = crate::decode::<EffectDto>(bytes) else {
		return Ok(None);
	};
	if dto.channel_id.0 == 0 || dto.user_id.0 == 0 || dto.sound_id.0 == 0 {
		return Ok(None);
	}
	let emoji = dto.emoji.and_then(|emoji| {
		let emoji = model::ReactionEmoji {
			id: emoji.id.filter(|id| id.0 != 0),
			name: emoji.name.filter(|name| !name.is_empty()),
		};
		emoji.valid().then_some(emoji)
	});
	Ok(Some(VoiceEffect {
		channel: dto.channel_id,
		user: dto.user_id,
		sound: dto.sound_id,
		volume: dto.sound_volume.unwrap_or(1.0).clamp(0.0, 1.0),
		emoji,
	}))
}

/// `VOICE_CHANNEL_STATUS_UPDATE`. The payload names the channel `id`, not `channel_id`.
pub fn voice_status(bytes: &[u8]) -> Result<(Id, Option<String>), DecodeError> {
	#[derive(Deserialize)]
	struct StatusDto {
		id: Id,
		#[serde(default, deserialize_with = "crate::lossy::null_default")]
		status: String,
	}
	if bytes.len() > MAX_SOUNDBOARD_WIRE {
		return Err(DecodeError);
	}
	let dto: StatusDto = crate::decode(bytes)?;
	if dto.id.0 == 0 {
		return Err(DecodeError);
	}
	let status: String = dto.status.chars().take(model::MAX_STATUS_CHARS).collect();
	Ok((dto.id, (!status.trim().is_empty()).then_some(status)))
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

	#[test]
	fn emoji_only_effects_are_ignored_and_sound_effects_are_bounded() {
		let reaction = r#"{"channel_id":"2","guild_id":"3","user_id":"4","emoji":{"name":"🎉"}}"#;
		assert_eq!(voice_effect(reaction.as_bytes()).unwrap(), None);
		let effect = voice_effect(
			r#"{"channel_id":"2","guild_id":"3","user_id":"4","sound_id":"9","sound_volume":4.0,"emoji":{"id":"7","name":"airhorn","animated":true}}"#.as_bytes(),
		)
		.unwrap()
		.expect("sound effect");
		assert_eq!(effect.channel, Id(2));
		assert_eq!(effect.user, Id(4));
		assert_eq!(effect.sound, Id(9));
		assert_eq!(effect.volume, 1.0);
		assert_eq!(effect.emoji.as_ref().unwrap().id, Some(Id(7)));
		assert_eq!(
			effect.url(),
			"https://cdn.discordapp.com/soundboard-sounds/9"
		);
		assert!(effect.valid());
		assert_eq!(
			voice_effect(br#"{"channel_id":"2","user_id":"4","sound_id":"0"}"#).unwrap(),
			None
		);
	}

	#[test]
	fn voice_status_reads_the_channel_id_field_and_clears_on_null() {
		let (channel, status) =
			voice_status(br#"{"id":"5","guild_id":"3","status":"Watching a film"}"#).unwrap();
		assert_eq!(channel, Id(5));
		assert_eq!(status.as_deref(), Some("Watching a film"));
		assert_eq!(
			voice_status(br#"{"id":"5","guild_id":"3","status":null}"#).unwrap(),
			(Id(5), None)
		);
		let long = format!(r#"{{"id":"5","status":"{}"}}"#, "x".repeat(900));
		let (_, status) = voice_status(long.as_bytes()).unwrap();
		assert_eq!(status.unwrap().chars().count(), model::MAX_STATUS_CHARS);
		assert!(voice_status(br#"{"id":"0","status":"nope"}"#).is_err());
	}
}
