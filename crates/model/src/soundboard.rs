//! Soundboard sounds offered by the picker. Display-only; never persisted.
use crate::Id;

/// Most sounds listed across the default and guild catalogs.
pub const MAX_SOUNDBOARD_SOUNDS: usize = 256;
/// Longest accepted sound name (the service allows 2-32 characters).
pub const MAX_SOUND_NAME: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct SoundboardSound {
	pub id: Id,
	pub name: String,
	/// 0.0..=1.0 as sent by the service.
	pub volume: f32,
	pub emoji_id: Option<Id>,
	pub emoji_name: Option<String>,
	/// None for the shared default catalog.
	pub guild: Option<Id>,
	pub available: bool,
}
impl SoundboardSound {
	pub fn valid(&self) -> bool {
		self.id.0 != 0
			&& (1..=MAX_SOUND_NAME).contains(&self.name.chars().count())
			&& self.name.len() <= MAX_SOUND_NAME * 4
			&& !self.name.chars().any(char::is_control)
			&& self.volume.is_finite()
			&& (0.0..=1.0).contains(&self.volume)
			&& self.emoji_id.is_none_or(|id| id.0 != 0)
			&& self
				.emoji_name
				.as_ref()
				.is_none_or(|name| !name.is_empty() && name.chars().count() <= 64)
	}
	pub fn heap_bytes(&self) -> usize {
		self.name.capacity() + self.emoji_name.as_ref().map_or(0, String::capacity)
	}
}
