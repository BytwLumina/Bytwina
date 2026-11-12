//! Owned collectibles offered by the profile decoration picker. Bounded and session-only.
use crate::auth::Failure;
use model::OwnedCollectible;

#[derive(Default)]
pub struct Picker {
	pub owned: Vec<OwnedCollectible>,
	pub request: u64,
	pub busy: bool,
	/// Distinguishes a successful empty response from a list that has not been loaded.
	pub loaded: bool,
	pub error: Option<&'static str>,
}
impl Picker {
	pub fn bytes(&self) -> usize {
		self.owned.iter().map(OwnedCollectible::heap_bytes).sum()
	}
	pub fn begin(&mut self, request: u64) {
		self.request = request;
		self.busy = true;
		self.error = None;
	}
	pub fn finish(&mut self, request: u64, result: Result<Vec<OwnedCollectible>, Failure>) {
		if !self.busy || request != self.request {
			return;
		}
		self.busy = false;
		match result {
			Ok(owned) => {
				self.owned = owned;
				self.loaded = true;
				self.error = None;
			}
			Err(failure) => {
				self.owned.clear();
				self.loaded = false;
				self.error = Some(failure.label());
			}
		}
	}
	pub fn clear(&mut self) {
		self.owned.clear();
		self.request = self.request.wrapping_add(1);
		self.busy = false;
		self.loaded = false;
		self.error = None;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{Command, Envelope, Event, State};

	#[test]
	fn rejected_owned_collectibles_can_retry_without_stale_outcomes() {
		let mut state = State::default();
		state.collectibles.begin(1);
		state.command_rejected(Command::OwnedCollectibles { request: 1 });
		assert!(!state.collectibles.busy);
		assert!(!state.collectibles.loaded);
		assert!(state.collectibles.error.is_some());

		state.collectibles.begin(2);
		state.command_rejected(Command::OwnedCollectibles { request: 1 });
		state.apply(Envelope {
			generation: state.generation,
			event: Event::OwnedCollectibles {
				request: 1,
				result: Ok(Vec::new()),
			},
		});
		assert!(state.collectibles.busy);
		assert!(state.collectibles.error.is_none());
		state.apply(Envelope {
			generation: state.generation,
			event: Event::OwnedCollectibles {
				request: 2,
				result: Ok(Vec::new()),
			},
		});
		assert!(!state.collectibles.busy && state.collectibles.loaded);
		assert!(state.collectibles.error.is_none());
	}

	#[test]
	fn empty_result_is_loaded_and_stale_results_cannot_replace_it() {
		let mut picker = Picker::default();
		picker.begin(1);
		picker.finish(1, Ok(Vec::new()));
		assert!(picker.loaded && !picker.busy && picker.owned.is_empty());
		picker.finish(1, Err(Failure::Network));
		assert!(picker.loaded && picker.error.is_none());
		picker.begin(2);
		picker.clear();
		picker.finish(2, Ok(Vec::new()));
		assert!(!picker.loaded && !picker.busy && picker.error.is_none());
		picker.begin(4);
		picker.finish(4, Err(Failure::Network));
		assert!(!picker.loaded && picker.error.is_some());
	}
}
