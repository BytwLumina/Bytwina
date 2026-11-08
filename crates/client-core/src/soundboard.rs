//! Soundboard catalog and voice channel statuses. Bounded and session-only.
use crate::auth::Failure;
use model::{Id, SoundboardSound};
use std::collections::BTreeMap;

/// Most channel statuses remembered at once; the oldest is evicted.
const MAX_STATUSES: usize = 512;
/// Longest accepted status text, matching the service's 500 character ceiling.
pub const MAX_STATUS_CHARS: usize = 500;

#[derive(Default)]
pub struct Soundboard {
	pub sounds: Vec<SoundboardSound>,
	pub request: u64,
	pub busy: bool,
	pub error: Option<&'static str>,
	pub query: String,
	/// The sound whose send is in flight, so the tile can show progress.
	pub sending: Option<Id>,
	pub send_request: u64,
}
impl Soundboard {
	pub fn bytes(&self) -> usize {
		self.sounds.iter().map(SoundboardSound::heap_bytes).sum()
	}
	pub fn begin(&mut self, request: u64) {
		self.request = request;
		self.busy = true;
		self.error = None;
	}
	pub fn finish(&mut self, request: u64, result: Result<Vec<SoundboardSound>, Failure>) {
		if request != self.request {
			return;
		}
		self.busy = false;
		match result {
			Ok(sounds) => {
				self.sounds = sounds;
				self.error = None;
			}
			Err(failure) => {
				self.sounds.clear();
				self.error = Some(failure.label());
			}
		}
	}
	pub fn sent(&mut self, request: u64, result: Result<(), Failure>) {
		if request != self.send_request {
			return;
		}
		self.sending = None;
		self.error = result.err().map(|failure| failure.label());
	}
	pub fn clear(&mut self) {
		self.sounds.clear();
		self.busy = false;
		self.sending = None;
		self.error = None;
	}
	/// Usable sounds matching the query, defaults first then the guild's own.
	pub fn matches(&self) -> Vec<&SoundboardSound> {
		let query = self.query.trim().to_lowercase();
		let mut rows: Vec<&SoundboardSound> = self
			.sounds
			.iter()
			.filter(|sound| sound.available)
			.filter(|sound| query.is_empty() || sound.name.to_lowercase().contains(&query))
			.collect();
		rows.sort_by_key(|sound| (sound.guild.is_some(), sound.name.to_lowercase()));
		rows
	}
}

/// Voice channel statuses reported by the Gateway. Never persisted.
#[derive(Default)]
pub struct VoiceStatuses {
	rows: BTreeMap<Id, String>,
}
impl VoiceStatuses {
	pub fn get(&self, channel: Id) -> Option<&str> {
		self.rows.get(&channel).map(String::as_str)
	}
	pub fn set(&mut self, channel: Id, status: Option<String>) {
		match status {
			Some(status) if !status.trim().is_empty() => {
				let status: String = status.chars().take(MAX_STATUS_CHARS).collect();
				if !self.rows.contains_key(&channel) && self.rows.len() >= MAX_STATUSES {
					if let Some(oldest) = self.rows.keys().next().copied() {
						self.rows.remove(&oldest);
					}
				}
				self.rows.insert(channel, status);
			}
			_ => {
				self.rows.remove(&channel);
			}
		}
	}
	pub fn clear(&mut self) {
		self.rows.clear();
	}
	pub fn valid_status(status: &str) -> bool {
		status.chars().count() <= MAX_STATUS_CHARS && !status.chars().any(char::is_control)
	}
}
