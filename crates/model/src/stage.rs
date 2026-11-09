//! Guild scheduled events and stage instances.
//!
//! Discord documents the event object at
//! `docs.discord.food/resources/guild-scheduled-event` and the stage instance at
//! `docs.discord.food/resources/stage-instance`. Both are guild-scoped and live only in
//! memory: opening a guild reads its events, and the gateway keeps them current.

use crate::Id;

/// Events kept per guild. Discord has no published ceiling; the panel is a list, not a store.
pub const MAX_SCHEDULED_EVENTS: usize = 64;
/// Stage instances kept per guild; one per live stage channel.
pub const MAX_STAGE_INSTANCES: usize = 64;
/// Event name, 1-100 characters.
pub const MAX_EVENT_NAME_BYTES: usize = 400;
/// Event description, 1-1000 characters.
pub const MAX_EVENT_DESCRIPTION_BYTES: usize = 4000;
/// Event location, for external events.
pub const MAX_EVENT_LOCATION_BYTES: usize = 1024;
/// Stage topic, 1-100 characters.
pub const MAX_STAGE_TOPIC_BYTES: usize = 400;
/// Total heap held by one guild's events.
pub const MAX_EVENT_BYTES: usize = 256 * 1024;

/// Privacy level 1: visible to anyone who can reach the guild.
pub const PRIVACY_PUBLIC: u8 = 1;
/// Privacy level 2: visible to guild members only.
pub const PRIVACY_GUILD_ONLY: u8 = 2;

/// Where a scheduled event is hosted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduledEventEntity {
	/// The event runs in a stage channel.
	StageInstance,
	/// The event runs in a voice channel.
	Voice,
	/// The event happens somewhere else.
	External,
	/// The event is a prime time event.
	PrimeTime,
}
impl ScheduledEventEntity {
	pub fn from_wire(value: u8) -> Option<Self> {
		match value {
			1 => Some(Self::StageInstance),
			2 => Some(Self::Voice),
			3 => Some(Self::External),
			4 => Some(Self::PrimeTime),
			_ => None,
		}
	}
	pub fn wire(self) -> u8 {
		match self {
			Self::StageInstance => 1,
			Self::Voice => 2,
			Self::External => 3,
			Self::PrimeTime => 4,
		}
	}
}

/// Whether the event has started, finished or was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduledEventStatus {
	Scheduled,
	Active,
	Completed,
	Canceled,
}
impl ScheduledEventStatus {
	pub fn from_wire(value: u8) -> Option<Self> {
		match value {
			1 => Some(Self::Scheduled),
			2 => Some(Self::Active),
			3 => Some(Self::Completed),
			4 => Some(Self::Canceled),
			_ => None,
		}
	}
	pub fn wire(self) -> u8 {
		match self {
			Self::Scheduled => 1,
			Self::Active => 2,
			Self::Completed => 3,
			Self::Canceled => 4,
		}
	}
	/// Whether the event still belongs in the guild's events panel.
	pub fn listed(self) -> bool {
		matches!(self, Self::Scheduled | Self::Active)
	}
}

/// One guild scheduled event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledEvent {
	pub id: Id,
	pub guild_id: Id,
	/// The hosting channel; absent for an external event.
	pub channel_id: Option<Id>,
	pub creator_id: Option<Id>,
	pub name: String,
	pub description: Option<String>,
	/// Unix seconds.
	pub scheduled_start_time: Option<i64>,
	/// Unix seconds.
	pub scheduled_end_time: Option<i64>,
	pub privacy_level: u8,
	pub status: ScheduledEventStatus,
	pub entity_type: ScheduledEventEntity,
	/// The stage instance or channel the event is attached to.
	pub entity_id: Option<Id>,
	/// Where an external event takes place.
	pub location: Option<String>,
	/// Cover image hash.
	pub cover: Option<String>,
	/// How many members subscribed; only present when the read asked for it.
	pub user_count: u32,
	/// Whether this account subscribed to the event.
	pub interested: bool,
}
impl ScheduledEvent {
	pub fn heap_bytes(&self) -> usize {
		self.name.capacity()
			+ self.description.as_ref().map_or(0, String::capacity)
			+ self.location.as_ref().map_or(0, String::capacity)
			+ self.cover.as_ref().map_or(0, String::capacity)
	}
	pub fn valid(&self) -> bool {
		self.id.0 != 0
			&& self.guild_id.0 != 0
			&& !self.name.is_empty()
			&& self.name.len() <= MAX_EVENT_NAME_BYTES
			&& self
				.description
				.as_ref()
				.is_none_or(|text| text.len() <= MAX_EVENT_DESCRIPTION_BYTES)
			&& self
				.location
				.as_ref()
				.is_none_or(|text| text.len() <= MAX_EVENT_LOCATION_BYTES)
			&& self.cover.as_ref().is_none_or(|hash| hash.len() <= 128)
			&& matches!(self.privacy_level, PRIVACY_PUBLIC | PRIVACY_GUILD_ONLY)
			&& self.channel_id.is_none_or(|id| id.0 != 0)
			&& self.creator_id.is_none_or(|id| id.0 != 0)
			&& self.entity_id.is_none_or(|id| id.0 != 0)
			&& !(self.entity_type == ScheduledEventEntity::External && self.channel_id.is_some())
	}
	/// The channel the event is hosted in, when it is hosted in Discord at all.
	pub fn host_channel(&self) -> Option<Id> {
		match self.entity_type {
			ScheduledEventEntity::StageInstance | ScheduledEventEntity::Voice => {
				self.channel_id.or(self.entity_id)
			}
			ScheduledEventEntity::External | ScheduledEventEntity::PrimeTime => None,
		}
	}
}

/// The live stage in one stage channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageInstance {
	/// The stage channel's id; a stage instance is keyed by its channel.
	pub id: Id,
	pub guild_id: Id,
	/// The scheduled event that started this stage.
	pub guild_scheduled_event_id: Option<Id>,
	pub topic: String,
	pub privacy_level: u8,
	pub discoverable_disabled: bool,
	/// Unix seconds.
	pub scheduled_start_time: Option<i64>,
}
impl StageInstance {
	pub fn heap_bytes(&self) -> usize {
		self.topic.capacity()
	}
	pub fn valid(&self) -> bool {
		self.id.0 != 0
			&& self.guild_id.0 != 0
			&& !self.topic.is_empty()
			&& self.topic.len() <= MAX_STAGE_TOPIC_BYTES
			&& matches!(self.privacy_level, PRIVACY_PUBLIC | PRIVACY_GUILD_ONLY)
			&& self.guild_scheduled_event_id.is_none_or(|id| id.0 != 0)
	}
}

/// Heap held by a guild's events.
pub fn event_bytes(events: &Vec<ScheduledEvent>) -> usize {
	events.capacity() * std::mem::size_of::<ScheduledEvent>()
		+ events.iter().map(ScheduledEvent::heap_bytes).sum::<usize>()
}
/// Heap held by a guild's stage instances.
pub fn stage_bytes(instances: &Vec<StageInstance>) -> usize {
	instances.capacity() * std::mem::size_of::<StageInstance>()
		+ instances
			.iter()
			.map(StageInstance::heap_bytes)
			.sum::<usize>()
}
/// Whether a guild's events stay inside the documented ceilings.
pub fn valid_events(events: &Vec<ScheduledEvent>, max: usize) -> bool {
	events.len() <= max
		&& event_bytes(events) <= MAX_EVENT_BYTES
		&& events.iter().enumerate().all(|(i, event)| {
			event.valid() && !events[..i].iter().any(|other| other.id == event.id)
		})
}
/// Whether a guild's stage instances stay inside the documented ceilings.
pub fn valid_stage_instances(instances: &Vec<StageInstance>, max: usize) -> bool {
	instances.len() <= max
		&& stage_bytes(instances) <= MAX_EVENT_BYTES
		&& instances.iter().enumerate().all(|(i, stage)| {
			stage.valid() && !instances[..i].iter().any(|other| other.id == stage.id)
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn event(id: u64) -> ScheduledEvent {
		ScheduledEvent {
			id: Id(id),
			guild_id: Id(2),
			channel_id: Some(Id(3)),
			creator_id: Some(Id(4)),
			name: "Release".to_owned(),
			description: None,
			scheduled_start_time: Some(1_700_000_000),
			scheduled_end_time: None,
			privacy_level: PRIVACY_GUILD_ONLY,
			status: ScheduledEventStatus::Scheduled,
			entity_type: ScheduledEventEntity::Voice,
			entity_id: Some(Id(3)),
			location: None,
			cover: None,
			user_count: 3,
			interested: false,
		}
	}

	#[test]
	fn event_validation_rejects_impossible_combinations() {
		assert!(event(1).valid());
		let mut external = event(1);
		external.entity_type = ScheduledEventEntity::External;
		assert!(
			!external.valid(),
			"an external event must not name a channel"
		);
		external.channel_id = None;
		assert!(external.valid());
		let mut nameless = event(1);
		nameless.name = String::new();
		assert!(!nameless.valid());
		let mut private = event(1);
		private.privacy_level = 0;
		assert!(!private.valid());
		let mut zero = event(1);
		zero.id = Id(0);
		assert!(!zero.valid());
	}

	#[test]
	fn event_lists_are_bounded_by_count_and_bytes() {
		let events = vec![event(1), event(2)];
		assert!(valid_events(&events, 2));
		let mut duplicate = events.clone();
		duplicate.push(event(1));
		assert!(!valid_events(&duplicate, 8));
		let mut long = event(1);
		long.name = "x".repeat(MAX_EVENT_NAME_BYTES + 1);
		assert!(!long.valid());
	}

	#[test]
	fn stage_instances_reject_bad_topics_and_duplicates() {
		let stage = StageInstance {
			id: Id(3),
			guild_id: Id(2),
			guild_scheduled_event_id: Some(Id(9)),
			topic: "Keynote".to_owned(),
			privacy_level: PRIVACY_PUBLIC,
			discoverable_disabled: false,
			scheduled_start_time: None,
		};
		assert!(stage.valid());
		assert!(valid_stage_instances(&vec![stage.clone()], 1));
		let mut empty = stage.clone();
		empty.topic = String::new();
		assert!(!empty.valid());
		let mut long = stage;
		long.topic = "x".repeat(MAX_STAGE_TOPIC_BYTES + 1);
		assert!(!long.valid());
	}

	#[test]
	fn only_scheduled_and_active_events_stay_listed() {
		assert!(ScheduledEventStatus::Scheduled.listed());
		assert!(ScheduledEventStatus::Active.listed());
		assert!(!ScheduledEventStatus::Completed.listed());
		assert!(!ScheduledEventStatus::Canceled.listed());
	}
}
