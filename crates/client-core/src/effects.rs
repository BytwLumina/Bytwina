//! Short-lived voice-channel effects heard in the connected channel.
//!
//! A soundboard effect plays locally through the cue output and leaves a row that fades. Rows
//! are bounded and expire; nothing here is persisted or sent to anyone.

use model::Id;
use std::time::{Duration, Instant};

/// Rows kept at once; the oldest is dropped first.
const MAX_ROWS: usize = 8;
/// How long a row stays visible.
pub const TTL: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
	pub channel: Id,
	pub user: Id,
	pub sound: Id,
	/// The catalog name when it was loaded; empty when the sound is from another server.
	pub name: String,
	pub emoji: Option<model::ReactionEmoji>,
	pub volume: f32,
	pub at: Instant,
	/// The request the player reports back with, so one row never plays twice.
	pub request: u64,
}

#[derive(Default)]
pub struct Effects {
	rows: Vec<Row>,
	request: u64,
}

impl Effects {
	pub fn rows(&self) -> &[Row] {
		&self.rows
	}
	/// Rows still on screen, newest first.
	pub fn recent(&self, now: Instant) -> impl Iterator<Item = &Row> {
		self.rows
			.iter()
			.rev()
			.take_while(move |row| now.saturating_duration_since(row.at) < TTL)
	}
	/// Record one effect. Returns its request, or None for an unusable payload.
	pub fn push(&mut self, effect: &model::VoiceEffect, name: String, now: Instant) -> Option<u64> {
		if !effect.valid() {
			return None;
		}
		self.request = self.request.wrapping_add(1);
		let row = Row {
			channel: effect.channel,
			user: effect.user,
			sound: effect.sound,
			name,
			emoji: effect.emoji.clone(),
			volume: effect.volume,
			at: now,
			request: self.request,
		};
		if self.rows.len() >= MAX_ROWS {
			self.rows.remove(0);
		}
		self.rows.push(row);
		Some(self.request)
	}
	/// The newest row, and whether the player has not been handed it yet.
	pub fn unplayed(&self, played: Option<u64>, now: Instant) -> Option<(&Row, bool)> {
		let row = self.rows.last()?;
		(now.saturating_duration_since(row.at) < TTL).then_some((row, Some(row.request) != played))
	}
	/// Forget rows that left the screen so a long call holds only the visible ones.
	pub fn prune(&mut self, now: Instant) {
		self.rows
			.retain(|row| now.saturating_duration_since(row.at) < TTL);
	}
	pub fn clear(&mut self) {
		self.rows.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn effect(sound: u64) -> model::VoiceEffect {
		model::VoiceEffect {
			channel: Id(1),
			user: Id(2),
			sound: Id(sound),
			volume: 0.5,
			emoji: None,
		}
	}

	#[test]
	fn rows_expire_and_stay_bounded() {
		let now = Instant::now();
		let mut effects = Effects::default();
		for sound in 1..=(MAX_ROWS as u64 + 4) {
			assert!(effects.push(&effect(sound), String::new(), now).is_some());
		}
		assert_eq!(effects.rows().len(), MAX_ROWS);
		assert_eq!(effects.recent(now + TTL).count(), 0);
		effects.prune(now + TTL);
		assert!(effects.rows().is_empty());
	}

	#[test]
	fn a_row_is_handed_to_the_player_once() {
		let now = Instant::now();
		let mut effects = Effects::default();
		let request = effects.push(&effect(3), "Airhorn".into(), now).unwrap();
		assert_eq!(effects.unplayed(None, now).unwrap().0.request, request);
		assert!(effects.unplayed(Some(request), now).unwrap().1 == false);
		assert!(effects.unplayed(None, now + TTL).is_none());
	}

	#[test]
	fn an_unusable_effect_is_refused_without_spending_a_request() {
		let now = Instant::now();
		let mut effects = Effects::default();
		let mut zero = effect(0);
		assert!(effects.push(&zero, String::new(), now).is_none());
		zero.sound = Id(4);
		assert_eq!(effects.push(&zero, String::new(), now), Some(1));
	}
}
