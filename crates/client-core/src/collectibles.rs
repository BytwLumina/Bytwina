//! Owned collectibles offered by the profile decoration picker. Bounded and session-only.
use crate::auth::Failure;
use model::OwnedCollectible;

#[derive(Default)]
pub struct Picker {
	pub owned: Vec<OwnedCollectible>,
	pub request: u64,
	pub busy: bool,
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
		if request != self.request {
			return;
		}
		self.busy = false;
		match result {
			Ok(owned) => {
				self.owned = owned;
				self.error = None;
			}
			Err(failure) => {
				self.owned.clear();
				self.error = Some(failure.label());
			}
		}
	}
	pub fn clear(&mut self) {
		self.owned.clear();
		self.busy = false;
		self.error = None;
	}
}
