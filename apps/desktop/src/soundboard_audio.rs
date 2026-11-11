//! Soundboard output. A clip is downloaded by the attachment worker and played on the shared
//! cue path. Nothing here touches the voice engine, so the microphone and the call keep running
//! while a sound plays.
use crate::notification_sounds::{CueGate, MAX_CUE_BYTES, play_bytes};
use std::{
	collections::VecDeque,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::{Duration, Instant},
};

/// Clips waiting behind the one on the output; a burst of effects cannot pile up unbounded.
const MAX_QUEUED: usize = 4;

/// A decoded clip: what to play and which request it answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
	pub volume: f32,
	/// The request this clip answers, so a stale answer cannot report over a newer preview.
	pub request: u64,
}
/// The only URL a soundboard clip may come from; the host and path are not taken from a payload.
pub fn clip_url(sound: model::Id) -> Result<url::Url, &'static str> {
	if sound.0 == 0 {
		return Err("soundboard-ui-download-failed");
	}
	url::Url::parse(&format!(
		"https://cdn.discordapp.com/soundboard-sounds/{sound}"
	))
	.map_err(|_| "soundboard-ui-download-failed")
}

struct Playback {
	request: u64,
	// The stream stays alive until the clip drains or is replaced.
	// A test playback carries no stream, so the field is optional.
	_stream: Option<cpal::Stream>,
	gate: CueGate,
	deadline: Instant,
}

#[derive(Default)]
pub struct Player {
	generation: Arc<AtomicU64>,
	playing: Option<Playback>,
	queue: VecDeque<(Clip, Vec<u8>)>,
	/// Requests whose clip already ended or failed, oldest first.
	settled: VecDeque<(u64, &'static str)>,
}

impl Player {
	/// Silence everything: the current clip and every queued one.
	pub fn stop(&mut self) {
		self.generation.fetch_add(1, Ordering::AcqRel);
		self.playing = None;
		self.queue.clear();
	}

	/// Enqueue a downloaded clip. Returns false when the queue is full or the clip is unusable.
	pub fn enqueue_owned(&mut self, request: u64, volume: f32, bytes: Vec<u8>) -> bool {
		self.enqueue(Clip { volume, request }, bytes)
	}

	/// Enqueue a decoded clip. Returns false when the queue is full or the clip is unusable.
	pub fn enqueue(&mut self, clip: Clip, bytes: Vec<u8>) -> bool {
		if bytes.is_empty() || bytes.len() > MAX_CUE_BYTES || self.queue.len() >= MAX_QUEUED {
			return false;
		}
		self.queue.push_back((clip, bytes));
		true
	}

	/// Start the next queued clip when the output is free. Never blocks the caller.
	pub fn advance(&mut self) {
		if self.playing.is_some() {
			return;
		}
		let Some((clip, bytes)) = self.queue.pop_front() else {
			return;
		};
		let request = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
		let gate = CueGate::new(self.generation.clone(), request);
		match play_bytes(&bytes, gain(clip.volume), &gate) {
			Ok((stream, duration)) => {
				self.playing = Some(Playback {
					request: clip.request,
					_stream: Some(stream),
					gate,
					deadline: Instant::now() + duration + Duration::from_secs(2),
				});
			}
			Err(_) => self.settle(clip.request, "soundboard-ui-playback-failed"),
		}
	}

	/// The request that just finished or failed, with the failure key (empty on success).
	pub fn take_settled(&mut self) -> Option<(u64, &'static str)> {
		self.poll();
		self.settled.pop_front()
	}

	/// The request on the output now, if any.
	pub fn playing(&self) -> Option<u64> {
		self.playing.as_ref().map(|playback| playback.request)
	}

	/// Whether a clip is playing or waiting behind it.
	pub fn busy(&self) -> bool {
		self.playing.is_some() || !self.queue.is_empty()
	}

	/// Whether a request already ended, so its preview shows `Ended` instead of replaying.
	pub fn settled(&self, request: u64) -> bool {
		self.settled.iter().any(|(settled, _)| *settled == request)
	}

	fn settle(&mut self, request: u64, error: &'static str) {
		if self.settled.len() >= MAX_QUEUED * 2 {
			self.settled.pop_front();
		}
		self.settled.push_back((request, error));
	}

	fn poll(&mut self) {
		let Some(playing) = self.playing.as_ref() else {
			return;
		};
		let done = playing.gate.cancelled()
			|| playing.gate.finished()
			|| playing.gate.status() == 2
			|| Instant::now() >= playing.deadline;
		if !done {
			return;
		}
		let failed = playing.gate.status() == 2;
		let request = playing.request;
		self.playing = None;
		self.settle(
			request,
			if failed {
				"soundboard-ui-playback-failed"
			} else {
				""
			},
		);
	}
}

/// Clamp the service's own volume; a non-finite value plays at zero rather than full blast.
pub fn gain(volume: f32) -> f32 {
	if volume.is_finite() {
		volume.clamp(0.0, 1.0)
	} else {
		0.0
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	fn clip(request: u64) -> Clip {
		Clip {
			volume: 0.5,
			request,
		}
	}

	#[test]
	fn gain_is_finite_and_bounded() {
		assert_eq!(gain(f32::NAN), 0.0);
		assert_eq!(gain(-1.0), 0.0);
		assert_eq!(gain(2.0), 1.0);
		assert_eq!(gain(0.25), 0.25);
	}

	#[test]
	fn a_finished_or_expired_clip_settles_once_and_opens_the_queue() {
		let mut player = Player::default();
		player.playing = Some(Playback {
			request: 4,
			_stream: None,
			gate: CueGate::new(Arc::new(AtomicU64::new(1)), 1),
			deadline: Instant::now() + Duration::from_secs(30),
		});
		assert!(player.take_settled().is_none());
		player.playing.as_ref().unwrap().gate.finish_test();
		assert_eq!(player.take_settled(), Some((4, "")));
		assert!(player.settled(4));
		assert!(player.take_settled().is_none());
		player.playing = Some(Playback {
			request: 5,
			_stream: None,
			gate: CueGate::new(Arc::new(AtomicU64::new(1)), 1),
			deadline: Instant::now(),
		});
		assert_eq!(player.take_settled(), Some((5, "")));
	}

	#[test]
	fn the_queue_is_bounded_and_only_fixed_urls_are_built() {
		let mut player = Player::default();
		for request in 0..MAX_QUEUED as u64 {
			assert!(player.enqueue(clip(request), vec![1]));
		}
		assert!(!player.enqueue(clip(99), vec![1]));
		assert!(!player.enqueue(clip(1), Vec::new()));
		player.stop();
		assert!(player.queue.is_empty());
		assert_eq!(
			clip_url(model::Id(7)).unwrap().host_str(),
			Some("cdn.discordapp.com")
		);
		assert!(clip_url(model::Id(0)).is_err());
	}
}
