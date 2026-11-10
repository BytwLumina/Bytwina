//! Soundboard catalog and voice channel statuses. Bounded and session-only.
use crate::auth::Failure;
use model::{Id, SoundboardSound};
use std::collections::BTreeMap;

/// Most channel statuses remembered at once; the oldest is evicted.
const MAX_STATUSES: usize = 512;
/// Longest accepted status text, matching the service's 500 character ceiling.
pub const MAX_STATUS_CHARS: usize = model::MAX_STATUS_CHARS;

#[derive(Default)]
pub struct Soundboard {
	pub sounds: Vec<SoundboardSound>,
	pub guild: Option<Id>,
	pub request: u64,
	pub busy: bool,
	pub error: Option<&'static str>,
	pub query: String,
	/// The sound whose send is in flight, so the tile can show progress.
	pub sending: Option<Id>,
	pub send_request: u64,
	/// The clip being auditioned locally. Sending to the channel is a separate action.
	pub preview: Option<Preview>,
	preview_request: u64,
}
/// A local audition. The desktop owns fetching, decoding and the output device.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
	pub sound: Id,
	pub volume: f32,
	pub request: u64,
	pub state: PreviewState,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewState {
	#[default]
	Loading,
	Playing,
	Ended,
	Failed(&'static str),
}
impl Soundboard {
	/// Start auditioning one sound, replacing whatever was playing.
	pub fn begin_preview(&mut self, sound: &SoundboardSound) -> u64 {
		self.preview_request = self.preview_request.wrapping_add(1);
		self.preview = Some(Preview {
			sound: sound.id,
			volume: sound.volume,
			request: self.preview_request,
			state: PreviewState::Loading,
		});
		self.preview_request
	}
	pub fn preview_progress(&mut self, request: u64, state: PreviewState) {
		if let Some(preview) = &mut self.preview
			&& preview.request == request
		{
			preview.state = state;
		}
	}
	pub fn stop_preview(&mut self) {
		self.preview = None;
	}
	/// The catalog row for an id, so a remote effect can reuse a name already loaded.
	pub fn find(&self, sound: Id) -> Option<&SoundboardSound> {
		self.sounds.iter().find(|row| row.id == sound)
	}
	pub fn bytes(&self) -> usize {
		self.sounds.iter().map(SoundboardSound::heap_bytes).sum()
	}
	pub fn begin(&mut self, guild: Option<Id>, request: u64) {
		if self.guild != guild {
			self.sounds.clear();
			self.preview = None;
		}
		self.guild = guild;
		self.request = request;
		self.busy = true;
		self.error = None;
	}
	pub fn finish(&mut self, request: u64, result: Result<Vec<SoundboardSound>, Failure>) {
		if request != self.request || !self.busy {
			return;
		}
		self.busy = false;
		match result {
			Ok(sounds) => {
				let mut seen = std::collections::BTreeSet::new();
				self.sounds = sounds
					.into_iter()
					.filter(|sound| {
						sound.valid() && (sound.guild.is_none() || sound.guild == self.guild)
					})
					.filter(|sound| seen.insert(sound.id))
					.take(model::MAX_SOUNDBOARD_SOUNDS)
					.collect();
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
		self.request = self.request.wrapping_add(1);
		self.send_request = self.send_request.wrapping_add(1);
		self.sounds.clear();
		self.query.clear();
		self.busy = false;
		self.sending = None;
		self.error = None;
		self.preview = None;
	}
	/// Mark a sound as in flight; the tile disables until the answer settles.
	pub fn begin_send(&mut self, sound: Id, request: u64) {
		self.send_request = request;
		self.sending = Some(sound);
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
	/// Set when a status write settles, so the editor knows to close.
	pub saved: Option<(u64, Option<&'static str>)>,
}
impl VoiceStatuses {
	pub fn get(&self, channel: Id) -> Option<&str> {
		self.rows.get(&channel).map(String::as_str)
	}
	pub fn set(&mut self, channel: Id, status: Option<String>) {
		match status {
			Some(status) if !status.trim().is_empty() => {
				let status: String = status.chars().take(MAX_STATUS_CHARS).collect();
				if !self.rows.contains_key(&channel)
					&& self.rows.len() >= MAX_STATUSES
					&& let Some(oldest) = self.rows.keys().next().copied()
				{
					self.rows.remove(&oldest);
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
		self.saved = None;
	}
	/// Record the outcome of a status write for the editor that asked.
	pub fn settled(&mut self, request: u64, result: Result<(), crate::auth::Failure>) {
		self.saved = Some((request, result.err().map(|failure| failure.label())));
	}
	pub fn take_settled(&mut self) -> Option<(u64, Option<&'static str>)> {
		self.saved.take()
	}
	pub fn valid_status(status: &str) -> bool {
		status.chars().count() <= MAX_STATUS_CHARS && !status.chars().any(char::is_control)
	}
}

impl crate::State {
	pub fn can_send_soundboard(&self, channel: Id, sound: &SoundboardSound) -> bool {
		use model::permissions as p;
		let Some(call) = self.voice.active.as_ref() else {
			return false;
		};
		let Some(target) = self.channel(channel) else {
			return false;
		};
		sound.valid()
			&& sound.available
			&& target.kind == 2
			&& target.guild.is_some()
			&& call.channel == channel
			&& call.guild == target.guild
			&& !call.server_muted
			&& !call.server_deafened
			&& !call.deafened
			&& matches!(
				call.phase,
				crate::voice::Phase::Connected | crate::voice::Phase::Waiting
			) && self.soundboard.sending.is_none()
			// Speak carries the sound; without it the service would refuse after the popup
			// already showed a send in flight.
			&& self.permission(channel, p::VIEW_CHANNEL | p::CONNECT | p::SPEAK | p::USE_SOUNDBOARD)
				== Some(true)
			&& (sound.guild.is_none()
				|| sound.guild == target.guild
				|| self.permission(channel, p::USE_EXTERNAL_SOUNDS) == Some(true))
	}

	pub fn send_soundboard(&mut self, channel: Id, sound: Id) -> Option<crate::Command> {
		let row = self.soundboard.find(sound)?;
		if !self.can_send_soundboard(channel, row) {
			return None;
		}
		let source_guild = row
			.guild
			.filter(|guild| self.channel(channel).and_then(|c| c.guild) != Some(*guild));
		let request = self.soundboard.send_request.wrapping_add(1);
		self.soundboard.begin_send(sound, request);
		Some(crate::Command::SendSoundboardSound {
			channel,
			sound,
			source_guild,
			request,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn sound(id: u64, guild: Option<Id>) -> SoundboardSound {
		SoundboardSound {
			id: Id(id),
			name: "Sample".into(),
			volume: 0.5,
			emoji_id: None,
			emoji_name: None,
			guild,
			available: true,
		}
	}
	#[test]
	fn preview_does_not_invalidate_a_pending_send_or_reuse_a_stopped_request() {
		let mut board = Soundboard::default();
		board.begin_send(Id(1), 7);
		let first = board.begin_preview(&sound(1, None));
		board.sent(7, Ok(()));
		assert!(board.sending.is_none());
		board.stop_preview();
		let second = board.begin_preview(&sound(2, None));
		assert_ne!(first, second);
		board.preview_progress(first, PreviewState::Ended);
		assert_eq!(board.preview.as_ref().unwrap().state, PreviewState::Loading);
		board.preview_progress(second, PreviewState::Playing);
		assert_eq!(board.preview.as_ref().unwrap().state, PreviewState::Playing);
	}
	#[test]
	fn sending_needs_a_joined_voice_channel_and_an_available_sound() {
		let mut state = crate::State::default();
		let sound = sound(3, None);
		state.soundboard.sounds.push(sound.clone());
		// Not in a call: nothing to send into.
		assert!(!state.can_send_soundboard(Id(1), &sound));
		assert!(state.send_soundboard(Id(1), Id(3)).is_none());
		// An unavailable sound is refused even when the catalog still lists it.
		let mut hidden = sound;
		hidden.available = false;
		assert!(!state.can_send_soundboard(Id(1), &hidden));
	}

	#[test]
	fn catalog_switch_and_clear_reject_stale_results() {
		let mut board = Soundboard::default();
		board.begin(Some(Id(10)), 1);
		board.begin(Some(Id(20)), 2);
		board.finish(1, Ok(vec![sound(1, Some(Id(10)))]));
		assert!(board.busy && board.sounds.is_empty());
		board.finish(
			2,
			Ok(vec![
				sound(1, None),
				sound(1, None),
				sound(2, Some(Id(10))),
				sound(3, Some(Id(20))),
			]),
		);
		assert_eq!(board.sounds.len(), 2);
		board.begin(Some(Id(20)), 3);
		board.clear();
		board.finish(3, Ok(vec![sound(4, None)]));
		assert!(board.sounds.is_empty());
	}
}
