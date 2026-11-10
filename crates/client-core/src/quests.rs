//! Discord Quests.
//!
//! Discovery and progress are session memory only: the quest list is read over REST when the
//! panel opens, and the gateway keeps a status current. Nothing here is persisted, and the
//! list is bounded by `model::quests`.

use model::Id;

use crate::Command;

/// Whether the quest list is known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
	/// The list was never read.
	#[default]
	Idle,
	/// A read is in flight.
	Loading,
	/// The list is current.
	Ready,
	/// The last read failed or the service sent nothing usable.
	Unavailable,
}

/// What the quest panel learned.
#[derive(Clone, Debug)]
pub enum Event {
	/// A REST read replaced the whole list.
	Snapshot {
		request: u64,
		result: Result<model::quests::QuestList, &'static str>,
	},
	/// One quest was accepted; the reply is this account's new status.
	Enrolled {
		quest: Id,
		request: u64,
		result: Result<model::quests::UserStatus, &'static str>,
	},
	/// One reward was claimed; the reply is when the service granted it.
	Claimed {
		quest: Id,
		request: u64,
		result: Result<Option<i64>, &'static str>,
	},
	/// The gateway reported a status change for one quest.
	Status { status: model::quests::UserStatus },
}

/// Every quest this account can see, session memory only.
#[derive(Default)]
pub struct Quests {
	list: model::quests::QuestList,
	status: Status,
	error: Option<&'static str>,
	action_error: Option<&'static str>,
	sequence: u64,
	/// The read that is in flight; a late reply for another one is dropped.
	request: Option<u64>,
	/// The enrolment or claim in flight, with the quest it belongs to.
	action: Option<(Id, u64)>,
}

impl Quests {
	/// The quests as the service last listed them.
	pub fn list(&self) -> &[model::quests::Quest] {
		&self.list.quests
	}
	/// When the service allows enrolment again, if it blocked it.
	pub fn blocked_until(&self) -> Option<i64> {
		self.list.blocked_until
	}
	pub fn status(&self) -> Status {
		self.status
	}
	pub fn error(&self) -> Option<&'static str> {
		self.error.or(self.action_error)
	}
	pub fn loading(&self) -> bool {
		self.request.is_some()
	}
	pub fn busy(&self) -> bool {
		self.loading() || self.action.is_some()
	}
	/// Whether an enrolment or claim is in flight for this quest.
	pub fn action_pending(&self, quest: Id) -> bool {
		self.action.is_some_and(|(pending, _)| pending == quest)
	}
	/// A fresh request id, so a late reply cannot be mistaken for a newer one.
	pub fn next_request(&mut self) -> u64 {
		self.sequence = self.sequence.wrapping_add(1);
		self.sequence
	}
	/// Starts a read of the list.
	pub fn begin(&mut self, request: u64) {
		self.request = Some(request);
		self.status = Status::Loading;
	}
	/// Records the single action in flight. A second quest waits until this one settles.
	pub fn begin_action(&mut self, quest: Id, request: u64) -> bool {
		if self.action.is_some() {
			return false;
		}
		self.action = Some((quest, request));
		true
	}
	pub fn apply(&mut self, event: Event) {
		match event {
			Event::Snapshot { request, result } => {
				if self.request != Some(request) {
					return;
				}
				self.request = None;
				match result {
					Ok(list) => {
						let mut list = list;
						if list.quests.len() > model::quests::MAX_QUESTS {
							list.quests.truncate(model::quests::MAX_QUESTS);
						}
						self.list = list;
						self.status = Status::Ready;
						self.error = None;
					}
					Err(error) => {
						self.status = Status::Unavailable;
						self.error = Some(error);
					}
				}
			}
			Event::Enrolled {
				quest,
				request,
				result,
			} => {
				if self.action != Some((quest, request)) {
					return;
				}
				self.action = None;
				match result {
					Ok(status) => {
						if let Some(known) = self.quest_mut(quest) {
							status.apply_to(known);
						}
						self.error = None;
					}
					Err(error) => self.error = Some(error),
				}
			}
			Event::Claimed {
				quest,
				request,
				result,
			} => {
				if self.action != Some((quest, request)) {
					return;
				}
				self.action = None;
				match result {
					Ok(claimed_at) => {
						if let Some(known) = self.quest_mut(quest) {
							// A claim always means the quest is complete, even when the
							// reply omits the timestamp.
							known.completed_at = known.completed_at.or(claimed_at);
							known.claimed_at = claimed_at.or(known.completed_at);
						}
						self.error = None;
					}
					Err(error) => self.error = Some(error),
				}
			}
			Event::Status { status } => {
				// A quest that was never listed has no metadata to fold into.
				if let Some(quest) = status.quest
					&& let Some(known) = self.quest_mut(quest)
				{
					status.apply_to(known);
				}
			}
		}
	}
	fn quest_mut(&mut self, quest: Id) -> Option<&mut model::quests::Quest> {
		self.list.quests.iter_mut().find(|known| known.id == quest)
	}
}

impl super::State {
	/// The quests this account can see.
	pub fn quests(&self) -> &[model::quests::Quest] {
		self.quests.list()
	}
	pub fn quests_status(&self) -> Status {
		self.quests.status()
	}
	pub fn quests_error(&self) -> Option<&'static str> {
		self.quests.error()
	}
	pub fn quests_blocked_until(&self) -> Option<i64> {
		self.quests.blocked_until()
	}
	/// Reads the account's quests.
	pub fn request_quests(&mut self) -> Option<Command> {
		if self.quests.loading() {
			return None;
		}
		let request = self.quests.next_request();
		self.quests.begin(request);
		Some(Command::Quests { request })
	}
	/// Accepts one quest, unless it is unknown, already accepted or in flight.
	pub fn request_enroll_quest(&mut self, quest: Id) -> Option<Command> {
		if quest.0 == 0 || self.quests.action_pending(quest) {
			return None;
		}
		if self
			.quests
			.list()
			.iter()
			.find(|known| known.id == quest)?
			.status() != model::quests::Status::Available
		{
			return None;
		}
		let request = self.quests.next_request();
		if !self.quests.begin_action(quest, request) {
			return None;
		}
		Some(Command::EnrollQuest { quest, request })
	}
	/// Claims the reward of one completed quest.
	pub fn request_claim_quest(&mut self, quest: Id) -> Option<Command> {
		if quest.0 == 0 || self.quests.action_pending(quest) {
			return None;
		}
		let platform = self
			.quests
			.list()
			.iter()
			.find(|known| known.id == quest)
			.filter(|known| known.status() == model::quests::Status::Completed)?
			.platform;
		let request = self.quests.next_request();
		if !self.quests.begin_action(quest, request) {
			return None;
		}
		Some(Command::ClaimQuest {
			quest,
			platform,
			request,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn quest(id: u64) -> model::quests::Quest {
		model::quests::Quest {
			id: Id(id),
			name: "Kill the Aliens".to_owned(),
			game: "Cyberalien 2077".to_owned(),
			publisher: "Alien Studios".to_owned(),
			application: Id(7),
			starts_at: None,
			expires_at: None,
			hero: None,
			primary: None,
			secondary: None,
			tasks: vec![model::quests::Task {
				kind: "PLAY_ON_DESKTOP".to_owned(),
				target: 900,
				value: 0,
				completed: false,
			}],
			reward: Some("Cybernetic Headgear".to_owned()),
			platform: model::quests::PLATFORM_CROSS,
			enrolled_at: None,
			completed_at: None,
			claimed_at: None,
		}
	}

	fn loaded() -> Quests {
		let mut quests = Quests::default();
		quests.begin(1);
		quests.apply(Event::Snapshot {
			request: 1,
			result: Ok(model::quests::QuestList {
				quests: vec![quest(9)],
				blocked_until: None,
			}),
		});
		quests
	}

	#[test]
	fn a_snapshot_replaces_the_list_and_a_stale_read_is_dropped() {
		let mut quests = loaded();
		assert_eq!(quests.status(), Status::Ready);
		quests.begin(2);
		quests.apply(Event::Snapshot {
			request: 1,
			result: Ok(model::quests::QuestList::default()),
		});
		assert_eq!(quests.list().len(), 1, "a late reply is ignored");
		quests.apply(Event::Snapshot {
			request: 2,
			result: Ok(model::quests::QuestList::default()),
		});
		assert!(quests.list().is_empty());
	}

	#[test]
	fn a_failed_read_is_not_offered_as_current() {
		let mut quests = Quests::default();
		quests.begin(1);
		quests.apply(Event::Snapshot {
			request: 1,
			result: Err("quests-panel-unavailable"),
		});
		assert_eq!(quests.status(), Status::Unavailable);
		assert_eq!(quests.error(), Some("quests-panel-unavailable"));
		assert!(!quests.loading());
	}

	#[test]
	fn an_enrolment_folds_the_reply_into_the_quest() {
		let mut quests = loaded();
		quests.begin_action(Id(9), 2);
		quests.apply(Event::Enrolled {
			quest: Id(9),
			request: 2,
			result: Ok(model::quests::UserStatus {
				quest: Some(Id(9)),
				enrolled_at: Some(10),
				completed_at: None,
				claimed_at: None,
				progress: vec![],
			}),
		});
		assert_eq!(quests.list()[0].status(), model::quests::Status::Enrolled);
		assert!(!quests.action_pending(Id(9)));
	}

	#[test]
	fn a_late_action_reply_does_not_touch_a_newer_one() {
		let mut quests = loaded();
		assert!(quests.begin_action(Id(9), 5));
		quests.apply(Event::Enrolled {
			quest: Id(9),
			request: 4,
			result: Ok(model::quests::UserStatus {
				quest: Some(Id(9)),
				enrolled_at: Some(10),
				completed_at: None,
				claimed_at: None,
				progress: vec![],
			}),
		});
		assert_eq!(quests.list()[0].status(), model::quests::Status::Available);
		assert!(quests.action_pending(Id(9)));
	}

	#[test]
	fn a_claim_leaves_the_quest_claimed() {
		let mut quests = loaded();
		quests.list.quests[0].completed_at = Some(20);
		assert!(quests.begin_action(Id(9), 3));
		quests.apply(Event::Claimed {
			quest: Id(9),
			request: 3,
			result: Ok(Some(30)),
		});
		assert_eq!(quests.list()[0].claimed_at, Some(30));
		assert_eq!(quests.list()[0].status(), model::quests::Status::Claimed);
	}

	#[test]
	fn a_gateway_status_only_updates_a_listed_quest() {
		let mut quests = loaded();
		let status = model::quests::UserStatus {
			quest: Some(Id(9)),
			enrolled_at: Some(10),
			completed_at: Some(20),
			claimed_at: None,
			progress: vec![model::quests::TaskProgress {
				kind: "PLAY_ON_DESKTOP".to_owned(),
				value: 900,
				completed: false,
			}],
		};
		quests.apply(Event::Status {
			status: status.clone(),
		});
		assert!(quests.list()[0].tasks[0].completed);
		quests.apply(Event::Status {
			status: model::quests::UserStatus {
				quest: Some(Id(10)),
				..status
			},
		});
		assert_eq!(quests.list().len(), 1);
	}

	#[test]
	fn only_a_completed_quest_can_be_claimed() {
		let mut state = super::super::State {
			quests: loaded(),
			..Default::default()
		};
		state.quests.list.quests[0].enrolled_at = Some(1);
		assert!(state.request_claim_quest(Id(9)).is_none());
		state.quests.list.quests[0].completed_at = Some(2);
		let command = state.request_claim_quest(Id(9)).unwrap();
		assert!(matches!(command, Command::ClaimQuest { quest, .. } if quest == Id(9)));
		assert!(
			state.request_claim_quest(Id(9)).is_none(),
			"already in flight"
		);
	}

	#[test]
	fn an_action_does_not_stale_its_running_read_and_only_one_action_runs() {
		let mut quests = Quests::default();
		let read = quests.next_request();
		quests.begin(read);
		let action = quests.next_request();
		assert!(quests.begin_action(Id(1), action));
		assert!(!quests.begin_action(Id(2), quests.next_request()));
		quests.apply(Event::Snapshot {
			request: read,
			result: Ok(model::quests::QuestList::default()),
		});
		assert!(!quests.loading());
		assert!(quests.action_pending(Id(1)));
	}
}
