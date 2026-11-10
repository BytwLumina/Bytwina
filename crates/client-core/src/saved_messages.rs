//! Saved messages and reminders. Session memory only; the service owns the real list.
//!
//! The previous implementation let a slower list overwrite a confirmed save and could not
//! tell a failed write from a stale one, so a lost answer stuck the row. Reads and writes are
//! now sequenced: any write marks the list stale, and only a read issued afterwards may
//! replace it.

use crate::auth::Failure;
use model::{Id, saved_messages::MAX_SAVED_MESSAGES, saved_messages::SavedMessage};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
	Load {
		request: u64,
	},
	Save {
		channel: Id,
		message: Id,
		due_at: Option<i64>,
		request: u64,
	},
	Remove {
		channel: Id,
		message: Id,
		request: u64,
	},
}

#[derive(Clone, Debug)]
pub enum Event {
	List {
		request: u64,
		result: Result<Vec<SavedMessage>, Failure>,
	},
	Saved {
		channel: Id,
		message: Id,
		request: u64,
		result: Result<SavedMessage, Failure>,
	},
	Removed {
		request: u64,
		result: Result<(Id, Id), Failure>,
	},
}

#[derive(Default)]
pub struct SavedMessages {
	rows: BTreeMap<(Id, Id), SavedMessage>,
	/// Bumped by every confirmed write; a list loaded before it cannot replace the rows.
	write_epoch: u64,
	list_epoch: u64,
	loading: bool,
	list_request: u64,
	request: u64,
	/// The write in flight, so a second one cannot race it.
	pending: Option<(u64, (Id, Id))>,
	error: Option<&'static str>,
	list_error: Option<&'static str>,
	/// A `message:null` row is removable but its content is unknown.
	pub query: String,
}

impl SavedMessages {
	pub fn rows(&self) -> impl Iterator<Item = &SavedMessage> {
		self.rows.values()
	}
	pub fn len(&self) -> usize {
		self.rows.len()
	}
	pub fn is_empty(&self) -> bool {
		self.rows.is_empty()
	}
	pub fn loading(&self) -> bool {
		self.loading
	}
	pub fn pending(&self, channel: Id, message: Id) -> bool {
		self.pending
			.is_some_and(|(_, key)| key == (channel, message))
	}
	pub fn busy(&self) -> bool {
		self.pending.is_some()
	}
	pub fn error(&self) -> Option<&'static str> {
		self.error
	}
	pub fn list_error(&self) -> Option<&'static str> {
		self.list_error
	}
	pub fn saved(&self, channel: Id, message: Id) -> Option<&SavedMessage> {
		self.rows.get(&(channel, message))
	}
	pub fn next(&mut self) -> u64 {
		self.request = self.request.wrapping_add(1);
		self.request
	}
	pub fn begin_list(&mut self, request: u64) {
		self.loading = true;
		self.list_request = request;
		self.list_epoch = self.write_epoch;
		self.list_error = None;
	}
	pub fn begin_write(&mut self, request: u64, channel: Id, message: Id) {
		self.request = request;
		self.pending = Some((request, (channel, message)));
		self.error = None;
	}
	pub fn apply(&mut self, event: Event) {
		match event {
			Event::List { request, result } => self.list(request, result),
			Event::Saved {
				channel,
				message,
				request,
				result,
			} => {
				if !self.settle(request, channel, message) {
					return;
				}
				match result {
					Ok(row) if row.key() == (channel, message) => {
						self.write_epoch += 1;
						self.rows.insert((channel, message), row);
					}
					Ok(_) => self.error = Some("saved-messages-answer-mismatch"),
					Err(failure) => self.error = Some(failure.label()),
				}
			}
			Event::Removed { request, result } => {
				let Some((_, key)) = self.pending else {
					return;
				};
				match &result {
					Ok(found) if *found != key => {
						if request == self.pending.map_or(0, |(id, _)| id) {
							self.error = Some("saved-messages-answer-mismatch");
						}
						return;
					}
					_ => {}
				}
				if !self.settle(request, key.0, key.1) {
					return;
				}
				match result {
					Ok((channel, message)) => {
						self.write_epoch += 1;
						self.rows.remove(&(channel, message));
					}
					Err(failure) => self.error = Some(failure.label()),
				}
			}
		}
	}
	fn settle(&mut self, request: u64, channel: Id, message: Id) -> bool {
		if self.pending != Some((request, (channel, message))) {
			return false;
		}
		self.pending = None;
		true
	}
	fn list(&mut self, request: u64, result: Result<Vec<SavedMessage>, Failure>) {
		if request != self.list_request || !self.loading {
			return;
		}
		self.loading = false;
		// A write confirmed while this read was in flight makes its rows stale.
		if self.list_epoch != self.write_epoch || self.pending.is_some() {
			self.loading = false;
			return;
		}
		match result {
			Ok(rows) => {
				self.rows = rows
					.into_iter()
					.filter(|row| row.validate().is_ok())
					.take(MAX_SAVED_MESSAGES)
					.map(|row| (row.key(), row))
					.collect();
				self.list_error = None;
			}
			Err(failure) => self.list_error = Some(failure.label()),
		}
	}
	pub fn clear(&mut self) {
		*self = Self {
			request: self.request,
			..Default::default()
		};
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use model::saved_messages::MessagePreview;

	fn row(id: u64, due_at: Option<i64>) -> SavedMessage {
		SavedMessage {
			channel: Id(1),
			message: Id(id),
			guild: None,
			saved_at: 1,
			due_at,
			author_summary: String::new(),
			channel_summary: String::new(),
			message_summary: String::new(),
			preview: Some(MessagePreview {
				author: "A".into(),
				content: "B".into(),
			}),
		}
	}

	#[test]
	fn a_list_started_before_a_write_cannot_replace_it() {
		let mut saved = SavedMessages::default();
		saved.begin_list(1);
		saved.begin_write(2, Id(1), Id(2));
		saved.apply(Event::Saved {
			channel: Id(1),
			message: Id(2),
			request: 2,
			result: Ok(row(2, None)),
		});
		saved.apply(Event::List {
			request: 1,
			result: Ok(vec![row(3, None)]),
		});
		assert!(saved.saved(Id(1), Id(2)).is_some());
		assert!(saved.saved(Id(1), Id(3)).is_none());
		saved.begin_list(3);
		saved.apply(Event::List {
			request: 3,
			result: Ok(vec![row(4, None)]),
		});
		assert_eq!(saved.len(), 1);
	}

	#[test]
	fn a_stale_answer_leaves_the_write_free_and_keeps_its_error() {
		let mut saved = SavedMessages::default();
		saved.begin_write(7, Id(1), Id(2));
		saved.apply(Event::Saved {
			channel: Id(1),
			message: Id(2),
			request: 8,
			result: Ok(row(2, None)),
		});
		assert!(saved.busy());
		assert!(saved.error().is_none());
		saved.apply(Event::Saved {
			channel: Id(1),
			message: Id(2),
			request: 7,
			result: Err(Failure::Network),
		});
		assert!(!saved.busy());
		assert!(saved.error().is_some());
		assert!(saved.is_empty());
	}

	#[test]
	fn an_unavailable_reminder_is_still_removable() {
		let mut saved = SavedMessages::default();
		let mut gone = row(5, Some(9));
		gone.preview = None;
		saved.begin_list(1);
		saved.apply(Event::List {
			request: 1,
			result: Ok(vec![gone]),
		});
		assert!(saved.saved(Id(1), Id(5)).is_some());
		saved.begin_write(2, Id(1), Id(5));
		saved.apply(Event::Removed {
			request: 2,
			result: Ok((Id(1), Id(5))),
		});
		assert!(saved.is_empty());
	}
}

impl crate::State {
	pub fn load_saved_messages(&mut self) -> Option<crate::Command> {
		if self.saved_messages.loading() {
			return None;
		}
		let request = self.saved_messages.next();
		self.saved_messages.begin_list(request);
		Some(crate::Command::SavedMessages(Command::Load { request }))
	}
	/// Save or re-time one message. `due_at` alone is sent, so unmentioned fields stay as they are.
	pub fn save_message(
		&mut self,
		channel: Id,
		message: Id,
		due_at: Option<i64>,
	) -> Option<crate::Command> {
		if self.saved_messages.busy()
			|| channel.0 == 0
			|| message.0 == 0
			|| due_at.is_some_and(|due| !model::saved_messages::valid_timestamp(due))
			|| self.permission(
				channel,
				model::permissions::VIEW_CHANNEL | model::permissions::READ_MESSAGE_HISTORY,
			) != Some(true)
		{
			return None;
		}
		let request = self.saved_messages.next();
		self.saved_messages.begin_write(request, channel, message);
		Some(crate::Command::SavedMessages(Command::Save {
			channel,
			message,
			due_at,
			request,
		}))
	}
	/// Removal stays available for a row whose message the client can no longer see.
	pub fn remove_saved_message(&mut self, channel: Id, message: Id) -> Option<crate::Command> {
		if self.saved_messages.busy() || self.saved_messages.saved(channel, message).is_none() {
			return None;
		}
		let request = self.saved_messages.next();
		self.saved_messages.begin_write(request, channel, message);
		Some(crate::Command::SavedMessages(Command::Remove {
			channel,
			message,
			request,
		}))
	}
}
