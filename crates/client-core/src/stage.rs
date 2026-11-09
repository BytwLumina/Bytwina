//! Guild scheduled events and live stages.
//!
//! The panel is session memory only: a guild's events are read over REST and kept current by
//! the gateway. Nothing here is persisted, and every collection is bounded by count and bytes
//! (see `model::stage`).

use model::Id;
use std::collections::BTreeMap;

use crate::Command;

/// Guilds that may keep a loaded event list at once.
pub const MAX_GUILDS: usize = 256;
/// Subscribers kept for one event; Discord pages the list by the hundred.
pub const MAX_SUBSCRIBERS: usize = 100;
/// Voice presences kept at once; one per member of every joined channel.
pub const MAX_PRESENCES: usize = 2048;

/// Whether a guild's event list is known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
	/// The guild has not been read yet.
	#[default]
	Idle,
	/// A read is in flight.
	Loading,
	/// The list is current.
	Ready,
	/// The last read failed or the service sent nothing usable.
	Unavailable,
}

/// What one member of a stage channel is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presence {
	/// The member sits in the audience, not on the stage.
	pub suppress: bool,
	/// The member asked to speak.
	pub request_to_speak: bool,
}

/// What the events panel learned.
#[derive(Clone, Debug)]
pub enum Event {
	/// A REST read replaced the guild's events. Live stages are gateway-only, so they
	/// stay as they are.
	Snapshot {
		guild: Id,
		events: Vec<model::ScheduledEvent>,
	},
	/// The gateway reported one event, created or updated.
	Upsert {
		guild: Id,
		event: model::ScheduledEvent,
	},
	/// The gateway reported that one event is gone.
	Remove { guild: Id, event: Id },
	/// The gateway reported one live stage, created or updated.
	Stage {
		guild: Id,
		stage: model::StageInstance,
	},
	/// The gateway reported that one stage ended.
	StageEnded { guild: Id, stage: Id },
	/// One member subscribed to or unsubscribed from an event.
	Subscriber {
		guild: Id,
		event: Id,
		member: Id,
		subscribed: bool,
	},
	/// The subscriber list of one event was read.
	Subscribers {
		guild: Id,
		event: Id,
		members: Vec<Id>,
	},
	/// This account's own interest in an event was set or cleared.
	Interest {
		guild: Id,
		event: Id,
		interested: bool,
		request: u64,
		result: Result<(), &'static str>,
	},
	/// A read of the guild's events failed.
	Failed {
		guild: Id,
		request: u64,
		error: &'static str,
	},
	/// One member of a voice channel moved, or asked to speak in a stage.
	Presence {
		guild: Id,
		channel: Id,
		user: Id,
		suppress: bool,
		request_to_speak: bool,
	},
	/// A stage action finished; `error` is set when it failed.
	Action {
		guild: Id,
		request: u64,
		error: Option<&'static str>,
	},
}

/// One guild's known events.
#[derive(Default)]
struct Guild {
	events: Vec<model::ScheduledEvent>,
	stages: Vec<model::StageInstance>,
	subscribers: BTreeMap<Id, Vec<Id>>,
	status: Status,
	/// The last failure the panel shows, if any.
	error: Option<&'static str>,
	loading: bool,
	/// The read that is in flight; a late reply for another one is dropped.
	request: u64,
	/// The interest request in flight, with the event it belongs to.
	interest: Option<(Id, u64)>,
}

/// Every guild's scheduled events, session memory only.
#[derive(Default)]
pub struct Events {
	guilds: BTreeMap<Id, Guild>,
	revision: u64,
	/// The request counter the panel hands to the API layer.
	request: u64,
	/// The stage presence of each member of each voice channel, by (channel, member).
	presence: BTreeMap<(Id, Id), Presence>,
}

impl Events {
	/// A fresh request id, so a late reply cannot be mistaken for a newer read.
	pub fn next_request(&mut self) -> u64 {
		self.request = self.request.wrapping_add(1);
		self.request
	}

	/// The guild's scheduled events, newest list first as the service sent it.
	pub fn events(&self, guild: Id) -> &[model::ScheduledEvent] {
		self.guilds.get(&guild).map_or(&[], |guild| &guild.events)
	}
	/// The guild's live stages.
	pub fn stages(&self, guild: Id) -> &[model::StageInstance] {
		self.guilds.get(&guild).map_or(&[], |guild| &guild.stages)
	}
	/// The members subscribed to one event.
	pub fn subscribers(&self, guild: Id, event: Id) -> &[Id] {
		self.guilds
			.get(&guild)
			.and_then(|guild| guild.subscribers.get(&event))
			.map_or(&[], Vec::as_slice)
	}
	/// Whether the guild's event list is known.
	pub fn status(&self, guild: Id) -> Status {
		self.guilds
			.get(&guild)
			.map_or(Status::Idle, |guild| guild.status)
	}
	/// The next event of a guild, by start time, for the guild banner.
	pub fn next_event(&self, guild: Id) -> Option<&model::ScheduledEvent> {
		self.guilds.get(&guild).and_then(|guild| {
			guild
				.events
				.iter()
				.filter(|event| event.status.listed())
				.min_by_key(|event| event.scheduled_start_time.unwrap_or(i64::MAX))
		})
	}
	/// The members of one voice channel, with the stage state of each one.
	pub fn presence(&self, channel: Id) -> Vec<(Id, Presence)> {
		self.presence
			.iter()
			.filter(|((where_, _), _)| *where_ == channel)
			.map(|((_, user), presence)| (*user, *presence))
			.collect()
	}
	/// The members on the stage of one channel.
	pub fn speakers(&self, channel: Id) -> Vec<Id> {
		self.presence(channel)
			.into_iter()
			.filter(|(_, presence)| !presence.suppress)
			.map(|(user, _)| user)
			.collect()
	}
	/// The members in the audience of one channel.
	pub fn audience(&self, channel: Id) -> Vec<Id> {
		self.presence(channel)
			.into_iter()
			.filter(|(_, presence)| presence.suppress)
			.map(|(user, _)| user)
			.collect()
	}
	/// The stage state of one member.
	pub fn member_presence(&self, channel: Id, user: Id) -> Option<Presence> {
		self.presence.get(&(channel, user)).copied()
	}
	/// The last failure the panel shows for this guild.
	pub fn error(&self, guild: Id) -> Option<&'static str> {
		self.guilds.get(&guild).and_then(|guild| guild.error)
	}
	/// A counter the panel reads to know that something changed.
	pub fn revision(&self) -> u64 {
		self.revision
	}
	/// Starts a read of one guild's events.
	pub fn begin(&mut self, guild: Id, request: u64) {
		let Some(slot) = self.guild(guild) else {
			return;
		};
		slot.loading = true;
		slot.request = request;
		slot.status = Status::Loading;
	}
	/// Whether a read of this guild is in flight.
	pub fn loading(&self, guild: Id) -> bool {
		self.guilds.get(&guild).is_some_and(|guild| guild.loading)
	}
	/// Records the interest request in flight for one event.
	pub fn begin_interest(&mut self, guild: Id, event: Id, request: u64) {
		let Some(slot) = self.guild(guild) else {
			return;
		};
		slot.interest = Some((event, request));
	}
	/// Whether an interest change is in flight for one event.
	pub fn interest_pending(&self, guild: Id, event: Id) -> bool {
		self.guilds
			.get(&guild)
			.is_some_and(|guild| guild.interest.is_some_and(|(id, _)| id == event))
	}
	/// Folds one event into the panel.
	pub fn apply(&mut self, event: Event) {
		self.revision = self.revision.wrapping_add(1);
		match event {
			Event::Snapshot { guild, events } => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				slot.loading = false;
				slot.events = events;
				slot.status = Status::Ready;
				slot.error = None;
			}
			Event::Upsert { guild, event } => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				match slot.events.iter_mut().find(|known| known.id == event.id) {
					Some(known) => {
						// Keep this account's own interest; the gateway does not report it.
						let mut event = event;
						event.interested = known.interested;
						*known = event;
					}
					None => {
						if slot.events.len() < model::MAX_SCHEDULED_EVENTS {
							slot.events.push(event);
						}
					}
				}
			}
			Event::Remove { guild, event } => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				slot.events.retain(|known| known.id != event);
				slot.subscribers.remove(&event);
			}
			Event::Stage { guild, stage } => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				match slot.stages.iter_mut().find(|known| known.id == stage.id) {
					Some(known) => *known = stage,
					None => {
						if slot.stages.len() < model::MAX_STAGE_INSTANCES {
							slot.stages.push(stage);
						}
					}
				}
			}
			Event::StageEnded { guild, stage } => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				slot.stages.retain(|known| known.id != stage);
			}
			Event::Subscriber {
				guild,
				event,
				member,
				subscribed,
			} => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				let members = slot.subscribers.entry(event).or_default();
				if subscribed {
					if !members.contains(&member) && members.len() < MAX_SUBSCRIBERS {
						members.push(member);
					}
				} else {
					members.retain(|known| *known != member);
				}
			}
			Event::Subscribers {
				guild,
				event,
				members,
			} => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				let mut members = members;
				members.truncate(MAX_SUBSCRIBERS);
				slot.subscribers.insert(event, members);
			}
			Event::Interest {
				guild,
				event,
				interested,
				request,
				result,
			} => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				if slot.interest != Some((event, request)) {
					// A newer interest change owns this event now.
					return;
				}
				slot.interest = None;
				match result {
					Ok(()) => {
						if let Some(known) = slot.events.iter_mut().find(|known| known.id == event)
						{
							known.interested = interested;
						}
					}
					Err(error) => {
						slot.status = Status::Unavailable;
						slot.error = Some(error);
					}
				}
			}
			Event::Presence {
				guild,
				channel,
				user,
				suppress,
				request_to_speak,
			} => {
				let _ = guild;
				if user.0 == 0 || channel.0 == 0 {
					return;
				}
				let key = (channel, user);
				if !self.presence.contains_key(&key) && self.presence.len() >= MAX_PRESENCES {
					return;
				}
				self.presence.insert(
					key,
					Presence {
						suppress,
						request_to_speak,
					},
				);
			}
			Event::Action {
				guild,
				request,
				error,
			} => {
				let _ = request;
				let Some(slot) = self.guild(guild) else {
					return;
				};
				slot.error = error;
			}
			Event::Failed {
				guild,
				request,
				error,
			} => {
				let Some(slot) = self.guild(guild) else {
					return;
				};
				if slot.request != request {
					return;
				}
				slot.loading = false;
				slot.status = Status::Unavailable;
				slot.error = Some(error);
			}
		}
	}
	/// The slot of one guild, created on first use. `None` when the panel already
	/// tracks the maximum number of guilds.
	fn guild(&mut self, guild: Id) -> Option<&mut Guild> {
		if !self.guilds.contains_key(&guild) && self.guilds.len() >= MAX_GUILDS {
			return None;
		}
		Some(self.guilds.entry(guild).or_default())
	}
}

impl super::State {
	/// The guild's scheduled events.
	pub fn guild_events(&self, guild: Id) -> &[model::ScheduledEvent] {
		self.events.events(guild)
	}
	/// The guild's live stages.
	pub fn guild_stages(&self, guild: Id) -> &[model::StageInstance] {
		self.events.stages(guild)
	}
	/// The members subscribed to one event.
	pub fn event_subscribers(&self, guild: Id, event: Id) -> &[Id] {
		self.events.subscribers(guild, event)
	}
	/// The guild's next event, for the guild banner.
	pub fn next_guild_event(&self, guild: Id) -> Option<&model::ScheduledEvent> {
		self.events.next_event(guild)
	}
	/// Whether the guild's event list is known.
	pub fn guild_events_status(&self, guild: Id) -> Status {
		self.events.status(guild)
	}
	/// Asks for one guild's scheduled events.
	pub fn request_scheduled_events(&mut self, guild: Id) -> Option<Command> {
		if guild.0 == 0 || self.events.loading(guild) {
			return None;
		}
		let request = self.events.next_request();
		self.events.begin(guild, request);
		Some(Command::ScheduledEvents { guild, request })
	}
	/// Sets or clears this account's interest in one event.
	pub fn request_event_interest(
		&mut self,
		guild: Id,
		event: Id,
		interested: bool,
	) -> Option<Command> {
		if guild.0 == 0 || event.0 == 0 || self.events.interest_pending(guild, event) {
			return None;
		}
		if !self
			.events
			.events(guild)
			.iter()
			.any(|known| known.id == event)
		{
			return None;
		}
		let request = self.events.next_request();
		self.events.begin_interest(guild, event, request);
		Some(Command::ScheduledEventInterest {
			guild,
			event,
			interested,
			request,
		})
	}
	/// Asks to speak in a stage channel, or takes the hand back.
	pub fn request_stage_speak(&mut self, guild: Id, channel: Id, asking: bool) -> Option<Command> {
		let user = self.user.as_ref()?.id;
		if guild.0 == 0 || channel.0 == 0 {
			return None;
		}
		let request = self.events.next_request();
		Some(Command::StageRequestToSpeak {
			guild,
			channel,
			user,
			asking,
			request,
		})
	}
	/// A host moves one member between the stage and the audience.
	///
	/// Discord requires the Mute Members permission for another member, so the check happens
	/// here rather than at the route.
	pub fn request_stage_moderate(
		&mut self,
		guild: Id,
		channel: Id,
		user: Id,
		speaker: bool,
	) -> Option<Command> {
		if guild.0 == 0 || channel.0 == 0 || user.0 == 0 {
			return None;
		}
		if self.permission(channel, model::permissions::MUTE_MEMBERS) != Some(true) {
			return None;
		}
		let request = self.events.next_request();
		Some(Command::StageModerate {
			guild,
			channel,
			user,
			speaker,
			request,
		})
	}
	/// Reads the members subscribed to one event.
	pub fn request_event_subscribers(&mut self, guild: Id, event: Id) -> Option<Command> {
		if guild.0 == 0 || event.0 == 0 {
			return None;
		}
		if !self
			.events
			.events(guild)
			.iter()
			.any(|known| known.id == event)
		{
			return None;
		}
		let request = self.events.next_request();
		Some(Command::ScheduledEventSubscribers {
			guild,
			event,
			request,
		})
	}
	/// A host starts a stage in one stage channel.
	///
	/// Discord asks for the Manage Channels permission, so the check happens here.
	pub fn request_stage_instance(
		&mut self,
		guild: Id,
		channel: Id,
		topic: &str,
	) -> Option<Command> {
		let topic = topic.trim();
		if guild.0 == 0 || channel.0 == 0 || topic.is_empty() {
			return None;
		}
		if topic.len() > model::MAX_STAGE_TOPIC_BYTES {
			return None;
		}
		if self.permission(channel, model::permissions::MANAGE_CHANNELS) != Some(true) {
			return None;
		}
		let request = self.events.next_request();
		Some(Command::StageInstanceCreate {
			guild,
			channel,
			topic: topic.to_owned(),
			request,
		})
	}
	/// A host ends the stage of one stage channel.
	pub fn request_stage_instance_end(&mut self, guild: Id, channel: Id) -> Option<Command> {
		if guild.0 == 0 || channel.0 == 0 {
			return None;
		}
		if self.permission(channel, model::permissions::MANAGE_CHANNELS) != Some(true) {
			return None;
		}
		let request = self.events.next_request();
		Some(Command::StageInstanceDelete {
			guild,
			channel,
			request,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn event(id: u64, interested: bool) -> model::ScheduledEvent {
		model::ScheduledEvent {
			id: Id(id),
			guild_id: Id(2),
			channel_id: Some(Id(3)),
			creator_id: None,
			name: "Release".to_owned(),
			description: None,
			scheduled_start_time: Some(1_700_000_000),
			scheduled_end_time: None,
			privacy_level: model::PRIVACY_GUILD_ONLY,
			status: model::ScheduledEventStatus::Scheduled,
			entity_type: model::ScheduledEventEntity::Voice,
			entity_id: Some(Id(3)),
			location: None,
			cover: None,
			user_count: 1,
			interested,
		}
	}

	#[test]
	fn a_snapshot_replaces_the_guild_list() {
		let mut events = Events::default();
		events.apply(Event::Snapshot {
			guild: Id(2),
			events: vec![event(9, false)],
		});
		assert_eq!(events.events(Id(2)).len(), 1);
		assert_eq!(events.status(Id(2)), Status::Ready);
		events.apply(Event::Snapshot {
			guild: Id(2),
			events: vec![],
		});
		assert!(events.events(Id(2)).is_empty());
	}

	#[test]
	fn an_update_keeps_this_accounts_own_interest() {
		let mut events = Events::default();
		events.apply(Event::Snapshot {
			guild: Id(2),
			events: vec![event(9, true)],
		});
		events.apply(Event::Upsert {
			guild: Id(2),
			event: event(9, false),
		});
		assert!(events.events(Id(2))[0].interested);
	}

	#[test]
	fn a_late_interest_reply_does_not_overwrite_a_newer_one() {
		let mut events = Events::default();
		events.apply(Event::Snapshot {
			guild: Id(2),
			events: vec![event(9, false)],
		});
		events.begin_interest(Id(2), Id(9), 4);
		events.apply(Event::Interest {
			guild: Id(2),
			event: Id(9),
			interested: true,
			request: 3,
			result: Ok(()),
		});
		assert!(!events.events(Id(2))[0].interested);
		events.apply(Event::Interest {
			guild: Id(2),
			event: Id(9),
			interested: true,
			request: 4,
			result: Ok(()),
		});
		assert!(events.events(Id(2))[0].interested);
	}

	#[test]
	fn subscribers_stay_bounded_and_unique() {
		let mut events = Events::default();
		for index in 0..(MAX_SUBSCRIBERS + 5) {
			events.apply(Event::Subscriber {
				guild: Id(2),
				event: Id(9),
				member: Id(index as u64 + 1),
				subscribed: true,
			});
		}
		assert_eq!(events.subscribers(Id(2), Id(9)).len(), MAX_SUBSCRIBERS);
		events.apply(Event::Subscriber {
			guild: Id(2),
			event: Id(9),
			member: Id(1),
			subscribed: false,
		});
		assert_eq!(events.subscribers(Id(2), Id(9)).len(), MAX_SUBSCRIBERS - 1);
	}

	#[test]
	fn a_failed_read_is_not_offered_as_current() {
		let mut events = Events::default();
		events.begin(Id(2), 1);
		assert!(events.loading(Id(2)));
		events.apply(Event::Failed {
			guild: Id(2),
			request: 1,
			error: "unavailable",
		});
		assert!(!events.loading(Id(2)));
		assert_eq!(events.status(Id(2)), Status::Unavailable);
	}
}
