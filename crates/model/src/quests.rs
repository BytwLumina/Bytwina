//! Discord Quests.
//!
//! The wire shape is documented only by the userdoccers project
//! (`docs.discord.food/resources/quests`) and is not part of Discord's public developer
//! documentation, so treat it as restricted and subject to change. Quests live only in
//! session memory; every list is bounded by count and bytes.

use crate::Id;

/// Quests kept at once. The discovery page is a list, not a store.
pub const MAX_QUESTS: usize = 64;
/// Tasks kept for one quest; the service sends at most a handful.
pub const MAX_TASKS: usize = 8;
/// Quest name, game title, publisher and reward name.
pub const MAX_QUEST_TEXT_BYTES: usize = 400;
/// A task type such as `PLAY_ON_DESKTOP`.
pub const MAX_TASK_TYPE_BYTES: usize = 64;
/// A CDN asset name or link.
pub const MAX_QUEST_ASSET_BYTES: usize = 512;
/// Total heap held by the quest list.
pub const MAX_QUEST_BYTES: usize = 512 * 1024;

/// The quest discovery page on desktop, the location an enrolment reports.
pub const CONTENT_QUEST_HOME_DESKTOP: u8 = 11;
/// Rewards redeemable on every platform.
pub const PLATFORM_CROSS: u8 = 0;
/// The highest platform value the service documents.
pub const PLATFORM_MAX: u8 = 4;

/// Where this account stands with one quest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
	/// Not accepted yet.
	#[default]
	Available,
	/// Accepted and still in progress.
	Enrolled,
	/// The task is done and the reward is claimable.
	Completed,
	/// The reward was claimed.
	Claimed,
}

/// One task of a quest and this account's progress on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
	/// The service's task type, e.g. `PLAY_ON_DESKTOP`.
	pub kind: String,
	/// The required value; seconds for a duration task.
	pub target: u64,
	/// This account's current value.
	pub value: u64,
	/// Whether the task reached its target.
	pub completed: bool,
}
impl Task {
	/// Progress as a fraction in `0.0..=1.0`, for the progress bar.
	pub fn fraction(&self) -> f32 {
		if self.target == 0 || self.completed {
			return if self.completed { 1.0 } else { 0.0 };
		}
		(self.value.min(self.target) as f32 / self.target as f32).clamp(0.0, 1.0)
	}
	pub fn heap_bytes(&self) -> usize {
		self.kind.capacity()
	}
}

/// One sponsored quest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quest {
	pub id: Id,
	/// `messages.quest_name`.
	pub name: String,
	/// `messages.game_title`.
	pub game: String,
	/// `messages.game_publisher`.
	pub publisher: String,
	/// The promoted application.
	pub application: Id,
	/// Unix seconds.
	pub starts_at: Option<i64>,
	/// Unix seconds.
	pub expires_at: Option<i64>,
	/// The hero image asset name, never a full remote URL we did not choose.
	pub hero: Option<String>,
	/// The gradient colours, as `0xRRGGBB`.
	pub primary: Option<u32>,
	pub secondary: Option<u32>,
	pub tasks: Vec<Task>,
	/// The first reward's name.
	pub reward: Option<String>,
	/// The platform the reward is claimed for.
	pub platform: u8,
	/// Unix seconds, from the user status.
	pub enrolled_at: Option<i64>,
	pub completed_at: Option<i64>,
	pub claimed_at: Option<i64>,
}
impl Quest {
	pub fn status(&self) -> Status {
		if self.claimed_at.is_some() {
			Status::Claimed
		} else if self.completed_at.is_some() {
			Status::Completed
		} else if self.enrolled_at.is_some() {
			Status::Enrolled
		} else {
			Status::Available
		}
	}
	pub fn heap_bytes(&self) -> usize {
		self.name.capacity()
			+ self.game.capacity()
			+ self.publisher.capacity()
			+ self.hero.as_ref().map_or(0, String::capacity)
			+ self.reward.as_ref().map_or(0, String::capacity)
			+ self.tasks.iter().map(Task::heap_bytes).sum::<usize>()
			+ self.tasks.capacity() * std::mem::size_of::<Task>()
	}
	pub fn valid(&self) -> bool {
		self.id.0 != 0
			&& !self.name.is_empty()
			&& self.name.len() <= MAX_QUEST_TEXT_BYTES
			&& self.game.len() <= MAX_QUEST_TEXT_BYTES
			&& self.publisher.len() <= MAX_QUEST_TEXT_BYTES
			&& self
				.reward
				.as_ref()
				.is_none_or(|text| text.len() <= MAX_QUEST_TEXT_BYTES)
			&& self
				.hero
				.as_ref()
				.is_none_or(|asset| !asset.is_empty() && asset.len() <= MAX_QUEST_ASSET_BYTES)
			&& self.platform <= PLATFORM_MAX
			&& self.tasks.len() <= MAX_TASKS
			&& self.tasks.iter().all(|task| {
				!task.kind.is_empty() && task.kind.len() <= MAX_TASK_TYPE_BYTES
			})
			// A claimed reward implies a completed quest.
			&& (self.claimed_at.is_none() || self.completed_at.is_some())
	}
	/// The next task that is still short of its target, for a one-line summary.
	pub fn current_task(&self) -> Option<&Task> {
		self.tasks.iter().find(|task| !task.completed)
	}
}

/// The user status object: what the service reports after an enrolment or a heartbeat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserStatus {
	/// The quest this status belongs to; present on gateway updates.
	pub quest: Option<Id>,
	pub enrolled_at: Option<i64>,
	pub completed_at: Option<i64>,
	pub claimed_at: Option<i64>,
	pub claimed_tier: Option<u32>,
	/// Progress keyed by task type, bounded at parse time.
	pub progress: Vec<TaskProgress>,
	/// Reward currency credited by the quest service.
	pub orb_quantity_claimed: Option<u64>,
	pub last_stream_heartbeat_at: Option<i64>,
	/// Stream/video seconds acknowledged by the service.
	pub stream_progress_seconds: u64,
	/// Quest-home/content areas hidden by the account.
	pub dismissed_quest_content: u32,
}
impl UserStatus {
	pub fn heap_bytes(&self) -> usize {
		self.progress
			.iter()
			.map(TaskProgress::heap_bytes)
			.sum::<usize>()
			+ std::mem::size_of_val(&self.dismissed_quest_content)
	}
	pub fn valid(&self) -> bool {
		self.quest.is_none_or(|id| id.0 != 0)
			&& self.progress.len() <= MAX_TASKS
			&& self.progress.iter().all(|progress| {
				!progress.kind.is_empty() && progress.kind.len() <= MAX_TASK_TYPE_BYTES
			}) && (self.claimed_at.is_none() || self.completed_at.is_some())
	}
	/// Folds this status into one quest, keeping the quest's own metadata.
	pub fn apply_to(&self, quest: &mut Quest) {
		quest.enrolled_at = self.enrolled_at.or(quest.enrolled_at);
		quest.completed_at = self.completed_at.or(quest.completed_at);
		quest.claimed_at = self.claimed_at.or(quest.claimed_at);
		for task in &mut quest.tasks {
			if let Some(progress) = self
				.progress
				.iter()
				.find(|progress| progress.kind.eq_ignore_ascii_case(&task.kind))
			{
				task.value = progress.value;
				task.completed =
					progress.completed || (task.target > 0 && task.value >= task.target);
			}
		}
	}
}

/// One task's progress inside a user status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskProgress {
	pub kind: String,
	pub value: u64,
	/// The service marked the task complete, even below its target.
	pub completed: bool,
}
impl TaskProgress {
	pub fn heap_bytes(&self) -> usize {
		self.kind.capacity()
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskHeartbeat {
	pub last_beat_at: Option<i64>,
	pub expires_at: Option<i64>,
}

/// The discovery list the service returns for this account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestList {
	pub quests: Vec<Quest>,
	/// When enrolment is allowed again; the service sets it after a blocked attempt.
	pub blocked_until: Option<i64>,
}

impl QuestList {
	pub fn bytes(&self) -> usize {
		self.quests.capacity() * std::mem::size_of::<Quest>()
			+ self.quests.iter().map(Quest::heap_bytes).sum::<usize>()
	}
}

/// Whether a quest list stays inside the documented ceilings.
pub fn valid_quests(list: &QuestList) -> bool {
	list.quests.len() <= MAX_QUESTS
		&& list.bytes() <= MAX_QUEST_BYTES
		&& list.quests.iter().enumerate().all(|(i, quest)| {
			quest.valid() && !list.quests[..i].iter().any(|other| other.id == quest.id)
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn quest(id: u64) -> Quest {
		Quest {
			id: Id(id),
			name: "Kill the Aliens".to_owned(),
			game: "Cyberalien 2077".to_owned(),
			publisher: "Alien Studios".to_owned(),
			application: Id(7),
			starts_at: Some(1_700_000_000),
			expires_at: Some(1_700_600_000),
			hero: Some("hero.jpg".to_owned()),
			primary: Some(0xE944D4),
			secondary: Some(0x5318A7),
			tasks: vec![Task {
				kind: "PLAY_ON_DESKTOP".to_owned(),
				target: 900,
				value: 0,
				completed: false,
				heartbeat: None,
			}],
			reward: Some("Cybernetic Headgear".to_owned()),
			platform: PLATFORM_CROSS,
			enrolled_at: None,
			completed_at: None,
			claimed_at: None,
		}
	}

	#[test]
	fn a_quest_list_is_bounded_by_count_and_bytes() {
		assert!(valid_quests(&QuestList {
			quests: vec![quest(1), quest(2)],
			blocked_until: None,
		}));
		let mut duplicate = QuestList {
			quests: vec![quest(1), quest(1)],
			blocked_until: None,
		};
		assert!(!valid_quests(&duplicate), "duplicate quest ids");
		duplicate.quests[1].id = Id(3);
		assert!(valid_quests(&duplicate));
		let mut long = QuestList {
			quests: vec![quest(1)],
			blocked_until: None,
		};
		long.quests[0].name = "x".repeat(MAX_QUEST_TEXT_BYTES + 1);
		assert!(!valid_quests(&long));
	}

	#[test]
	fn a_claim_implies_completion_and_sets_the_status() {
		let mut claimed = quest(1);
		claimed.claimed_at = Some(1_700_000_001);
		assert!(!claimed.valid(), "a claim without completion is refused");
		claimed.completed_at = Some(1_700_000_001);
		assert!(claimed.valid());
		assert_eq!(claimed.status(), Status::Claimed);
		let mut completed = quest(1);
		completed.completed_at = Some(1);
		assert_eq!(completed.status(), Status::Completed);
		let mut enrolled = quest(1);
		enrolled.enrolled_at = Some(1);
		assert_eq!(enrolled.status(), Status::Enrolled);
		assert_eq!(quest(1).status(), Status::Available);
	}

	#[test]
	fn task_progress_is_clamped_and_reflects_completion() {
		let mut task = Task {
			kind: "PLAY_ON_DESKTOP".to_owned(),
			target: 900,
			value: 450,
			completed: false,
		};
		assert!((task.fraction() - 0.5).abs() < f32::EPSILON);
		task.value = 2_000;
		task.completed = true;
		assert_eq!(task.fraction(), 1.0);
		task.completed = false;
		task.target = 0;
		assert_eq!(task.fraction(), 0.0);
	}

	#[test]
	fn a_status_update_folds_into_the_matching_task() {
		let mut quest = quest(1);
		let status = UserStatus {
			quest: Some(Id(1)),
			enrolled_at: Some(10),
			completed_at: Some(20),
			claimed_at: None,
			progress: vec![TaskProgress {
				kind: "play_on_desktop".to_owned(),
				value: 900,
				completed: false,
				heartbeat: None,
			}],
			..Default::default()
		};
		assert!(status.valid());
		status.apply_to(&mut quest);
		assert_eq!(quest.status(), Status::Completed);
		assert!(quest.tasks[0].completed);
		assert_eq!(quest.tasks[0].value, 900);
	}

	#[test]
	fn too_many_tasks_or_progress_rows_are_refused() {
		let mut loaded = quest(1);
		loaded.tasks = (0..=MAX_TASKS)
			.map(|index| Task {
				kind: format!("TASK_{index}"),
				target: 1,
				value: 0,
				completed: false,
			})
			.collect();
		assert!(!loaded.valid());
		let status = UserStatus {
			quest: Some(Id(1)),
			enrolled_at: Some(1),
			completed_at: None,
			claimed_at: None,
			progress: (0..=MAX_TASKS)
				.map(|index| TaskProgress {
					kind: format!("TASK_{index}"),
					value: 0,
					completed: false,
					heartbeat: None,
				})
				.collect(),
			..Default::default()
		};
		assert!(!status.valid());
	}
}
