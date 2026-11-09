//! Guild scheduled events and stage instances.
//!
//! Wire shapes follow `docs.discord.food/resources/guild-scheduled-event` and
//! `docs.discord.food/resources/stage-instance`. Every list is bounded while it is parsed,
//! so a hostile payload never grows past the documented ceilings.

use model::Id;
use serde::Deserialize;

/// Discord lists at most a hundred subscribers per page.
const MAX_SUBSCRIBERS: usize = 100;

/// Text refused at parse time, so an oversized name never reaches the model.
fn text<'de, D: serde::Deserializer<'de>, const MAX: usize>(d: D) -> Result<String, D::Error> {
	struct Text<const MAX: usize>;
	impl<'de, const MAX: usize> serde::de::Visitor<'de> for Text<MAX> {
		type Value = String;
		fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
			f.write_str("bounded text")
		}
		fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
			if value.len() > MAX {
				return Err(E::custom("Text capacity"));
			}
			Ok(value.to_owned())
		}
	}
	d.deserialize_str(Text::<MAX>)
}

/// Optional text, refused at parse time on the same bound.
fn optional_text<'de, D: serde::Deserializer<'de>, const MAX: usize>(
	d: D,
) -> Result<Option<String>, D::Error> {
	struct Text<const MAX: usize>;
	impl<'de, const MAX: usize> serde::de::Visitor<'de> for Text<MAX> {
		type Value = Option<String>;
		fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
			f.write_str("optional bounded text")
		}
		fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
			Ok(None)
		}
		fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
			Ok(None)
		}
		fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
			text::<D, MAX>(d).map(Some)
		}
	}
	d.deserialize_option(Text::<MAX>)
}

fn name_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
	text::<D, { model::MAX_EVENT_NAME_BYTES }>(d)
}

fn description_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	optional_text::<D, { model::MAX_EVENT_DESCRIPTION_BYTES }>(d)
}

fn location_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	optional_text::<D, { model::MAX_EVENT_LOCATION_BYTES }>(d)
}

fn cover_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	optional_text::<D, 128>(d)
}

fn topic_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
	text::<D, { model::MAX_STAGE_TOPIC_BYTES }>(d)
}

/// Discord sends RFC 3339 timestamps; the model keeps Unix seconds.
fn seconds(timestamp: Option<crate::Timestamp>) -> Option<i64> {
	timestamp
		.and_then(|value| value.unix_ms())
		.map(|millis| millis.div_euclid(1000))
}

/// The extra metadata an event carries; only the location is used today.
#[derive(Deserialize)]
pub struct EntityMetadataDto {
	#[serde(default, deserialize_with = "location_field")]
	pub location: Option<String>,
}

/// One guild scheduled event, as the service sends it.
#[derive(Deserialize)]
pub struct ScheduledEventDto {
	pub id: Id,
	pub guild_id: Id,
	#[serde(default)]
	pub channel_id: Option<Id>,
	#[serde(default)]
	pub creator_id: Option<Id>,
	#[serde(deserialize_with = "name_field")]
	pub name: String,
	#[serde(default, deserialize_with = "description_field")]
	pub description: Option<String>,
	#[serde(default)]
	pub scheduled_start_time: Option<crate::Timestamp>,
	#[serde(default)]
	pub scheduled_end_time: Option<crate::Timestamp>,
	#[serde(default)]
	pub privacy_level: u8,
	pub status: u8,
	pub entity_type: u8,
	#[serde(default)]
	pub entity_id: Option<Id>,
	#[serde(default)]
	pub entity_metadata: Option<EntityMetadataDto>,
	#[serde(default)]
	pub user_count: Option<u32>,
	#[serde(default, deserialize_with = "cover_field")]
	pub image: Option<String>,
}
impl ScheduledEventDto {
	pub fn into_model(self) -> Result<model::ScheduledEvent, crate::DecodeError> {
		let status =
			model::ScheduledEventStatus::from_wire(self.status).ok_or(crate::DecodeError)?;
		let entity_type =
			model::ScheduledEventEntity::from_wire(self.entity_type).ok_or(crate::DecodeError)?;
		let event = model::ScheduledEvent {
			id: self.id,
			guild_id: self.guild_id,
			channel_id: self.channel_id,
			creator_id: self.creator_id,
			name: self.name,
			description: self.description,
			scheduled_start_time: seconds(self.scheduled_start_time),
			scheduled_end_time: seconds(self.scheduled_end_time),
			privacy_level: self.privacy_level,
			status,
			entity_type,
			entity_id: self.entity_id,
			location: self.entity_metadata.and_then(|meta| meta.location),
			cover: self.image,
			user_count: self.user_count.unwrap_or(0),
			interested: false,
		};
		if !event.valid() {
			return Err(crate::DecodeError);
		}
		Ok(event)
	}
}

/// One live stage, as the service sends it.
#[derive(Deserialize)]
pub struct StageInstanceDto {
	pub id: Id,
	pub guild_id: Id,
	#[serde(default)]
	pub guild_scheduled_event_id: Option<Id>,
	#[serde(deserialize_with = "topic_field")]
	pub topic: String,
	#[serde(default)]
	pub privacy_level: u8,
	#[serde(default)]
	pub discoverable_disabled: bool,
	#[serde(default)]
	pub scheduled_start_time: Option<crate::Timestamp>,
}
impl StageInstanceDto {
	pub fn into_model(self) -> Result<model::StageInstance, crate::DecodeError> {
		let stage = model::StageInstance {
			id: self.id,
			guild_id: self.guild_id,
			guild_scheduled_event_id: self.guild_scheduled_event_id,
			topic: self.topic,
			privacy_level: self.privacy_level,
			discoverable_disabled: self.discoverable_disabled,
			scheduled_start_time: seconds(self.scheduled_start_time),
		};
		if !stage.valid() {
			return Err(crate::DecodeError);
		}
		Ok(stage)
	}
}

/// A guild's scheduled events, bounded while it is parsed.
pub struct ScheduledEvents(pub Vec<model::ScheduledEvent>);
impl<'de> serde::Deserialize<'de> for ScheduledEvents {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct List;
		impl<'de> serde::de::Visitor<'de> for List {
			type Value = ScheduledEvents;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a bounded scheduled event list")
			}
			fn visit_seq<A: serde::de::SeqAccess<'de>>(
				self,
				mut seq: A,
			) -> Result<Self::Value, A::Error> {
				let mut items = Vec::new();
				let mut total = 0usize;
				while let Some(dto) = seq.next_element::<ScheduledEventDto>()? {
					if items.len() >= model::MAX_SCHEDULED_EVENTS {
						return Err(serde::de::Error::custom("Too many scheduled events"));
					}
					let event = dto
						.into_model()
						.map_err(|_| serde::de::Error::custom("Invalid scheduled event"))?;
					total += event.heap_bytes();
					if total > model::MAX_EVENT_BYTES {
						return Err(serde::de::Error::custom("Scheduled event capacity"));
					}
					items.push(event);
				}
				Ok(ScheduledEvents(items))
			}
		}
		d.deserialize_seq(List)
	}
}

/// A guild's live stages, bounded while it is parsed.
pub struct StageInstances(pub Vec<model::StageInstance>);
impl<'de> serde::Deserialize<'de> for StageInstances {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct List;
		impl<'de> serde::de::Visitor<'de> for List {
			type Value = StageInstances;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a bounded stage instance list")
			}
			fn visit_seq<A: serde::de::SeqAccess<'de>>(
				self,
				mut seq: A,
			) -> Result<Self::Value, A::Error> {
				let mut items = Vec::new();
				let mut total = 0usize;
				while let Some(dto) = seq.next_element::<StageInstanceDto>()? {
					if items.len() >= model::MAX_STAGE_INSTANCES {
						return Err(serde::de::Error::custom("Too many stage instances"));
					}
					let stage = dto
						.into_model()
						.map_err(|_| serde::de::Error::custom("Invalid stage instance"))?;
					total += stage.heap_bytes();
					if total > model::MAX_EVENT_BYTES {
						return Err(serde::de::Error::custom("Stage instance capacity"));
					}
					items.push(stage);
				}
				Ok(StageInstances(items))
			}
		}
		d.deserialize_seq(List)
	}
}

/// A member's subscription to an event.
///
/// The gateway reports `user_id`; the REST subscriber list reports a nested `user` object.
#[derive(Deserialize)]
pub struct ScheduledEventUserDto {
	pub guild_scheduled_event_id: Id,
	#[serde(default)]
	pub user_id: Option<Id>,
	#[serde(default)]
	pub user: Option<ScheduledEventUserRefDto>,
	#[serde(default)]
	pub guild_id: Option<Id>,
}
impl ScheduledEventUserDto {
	/// The subscribed member, whichever shape the service used.
	pub fn member(&self) -> Option<Id> {
		self.user_id
			.or_else(|| self.user.as_ref().map(|user| user.id))
	}
}

/// The nested user the REST subscriber list reports.
#[derive(Deserialize)]
pub struct ScheduledEventUserRefDto {
	pub id: Id,
}

/// The members subscribed to one event, bounded while it is parsed.
pub struct ScheduledEventUsers(pub Vec<Id>);
impl<'de> serde::Deserialize<'de> for ScheduledEventUsers {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct List;
		impl<'de> serde::de::Visitor<'de> for List {
			type Value = ScheduledEventUsers;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a bounded scheduled event subscriber list")
			}
			fn visit_seq<A: serde::de::SeqAccess<'de>>(
				self,
				mut seq: A,
			) -> Result<Self::Value, A::Error> {
				let mut items = Vec::new();
				while let Some(dto) = seq.next_element::<ScheduledEventUserDto>()? {
					if items.len() >= MAX_SUBSCRIBERS {
						return Err(serde::de::Error::custom("Too many scheduled event users"));
					}
					let Some(member) = dto.member() else {
						return Err(serde::de::Error::custom(
							"Scheduled event user without an id",
						));
					};
					if member.0 == 0 {
						return Err(serde::de::Error::custom("Invalid scheduled event user"));
					}
					items.push(member);
				}
				Ok(ScheduledEventUsers(items))
			}
		}
		d.deserialize_seq(List)
	}
}

/// The members subscribed to one event.
pub fn event_subscribers(bytes: &[u8]) -> Result<Vec<Id>, crate::DecodeError> {
	let list = crate::decode::<ScheduledEventUsers>(bytes)?;
	Ok(list.0)
}

/// The guild's scheduled events, as the events panel reads them.
pub fn scheduled_events(bytes: &[u8]) -> Result<Vec<model::ScheduledEvent>, crate::DecodeError> {
	let list = crate::decode::<ScheduledEvents>(bytes)?;
	if !model::valid_events(&list.0, model::MAX_SCHEDULED_EVENTS) {
		return Err(crate::DecodeError);
	}
	Ok(list.0)
}

/// The guild's live stages.
pub fn stage_instances(bytes: &[u8]) -> Result<Vec<model::StageInstance>, crate::DecodeError> {
	let list = crate::decode::<StageInstances>(bytes)?;
	if !model::valid_stage_instances(&list.0, model::MAX_STAGE_INSTANCES) {
		return Err(crate::DecodeError);
	}
	Ok(list.0)
}

/// One scheduled event, as the gateway sends it.
pub fn scheduled_event(bytes: &[u8]) -> Result<model::ScheduledEvent, crate::DecodeError> {
	let dto = crate::decode::<ScheduledEventDto>(bytes)?;
	dto.into_model()
}

/// One live stage, as the gateway sends it.
pub fn stage_instance(bytes: &[u8]) -> Result<model::StageInstance, crate::DecodeError> {
	let dto = crate::decode::<StageInstanceDto>(bytes)?;
	dto.into_model()
}

/// The identity of a deleted scheduled event.
#[derive(Deserialize)]
pub struct ScheduledEventDeleteDto {
	pub id: Id,
	pub guild_id: Id,
}

/// The identity of a subscriber change.
#[derive(Deserialize)]
pub struct ScheduledEventUserEventDto {
	pub guild_scheduled_event_id: Id,
	pub user_id: Id,
	pub guild_id: Id,
}

/// The identity of a deleted stage instance.
#[derive(Deserialize)]
pub struct StageInstanceDeleteDto {
	pub id: Id,
	pub guild_id: Id,
}

/// A deleted scheduled event, as the gateway reports it.
pub fn scheduled_event_delete(bytes: &[u8]) -> Result<(Id, Id), crate::DecodeError> {
	let dto = crate::decode::<ScheduledEventDeleteDto>(bytes)?;
	Ok((dto.guild_id, dto.id))
}

/// A subscriber change, as the gateway reports it.
pub fn scheduled_event_user(bytes: &[u8]) -> Result<(Id, Id, Id), crate::DecodeError> {
	let dto = crate::decode::<ScheduledEventUserEventDto>(bytes)?;
	Ok((dto.guild_id, dto.guild_scheduled_event_id, dto.user_id))
}

/// A deleted stage instance, as the gateway reports it.
pub fn stage_instance_delete(bytes: &[u8]) -> Result<(Id, Id), crate::DecodeError> {
	let dto = crate::decode::<StageInstanceDeleteDto>(bytes)?;
	Ok((dto.guild_id, dto.id))
}

#[cfg(test)]
mod tests {
	use super::*;

	const EVENT: &str = r#"{"id":"9","guild_id":"2","channel_id":"3","creator_id":"4","name":"Release","description":"Ship it","scheduled_start_time":"2026-01-02T03:04:05+00:00","privacy_level":2,"status":1,"entity_type":2,"entity_id":"3","user_count":12}"#;

	#[test]
	fn a_scheduled_event_decodes_into_the_model() {
		let event = scheduled_event(EVENT.as_bytes()).unwrap();
		assert_eq!(event.id, Id(9));
		assert_eq!(event.guild_id, Id(2));
		assert_eq!(event.name, "Release");
		assert_eq!(event.description.as_deref(), Some("Ship it"));
		assert_eq!(event.status, model::ScheduledEventStatus::Scheduled);
		assert_eq!(event.entity_type, model::ScheduledEventEntity::Voice);
		assert_eq!(event.user_count, 12);
		assert_eq!(event.scheduled_start_time, Some(1_767_322_845));
		assert_eq!(event.host_channel(), Some(Id(3)));
	}

	#[test]
	fn an_external_event_keeps_its_location() {
		let raw = r#"{"id":"9","guild_id":"2","name":"Meetup","privacy_level":1,"status":1,"entity_type":3,"entity_metadata":{"location":"Town hall"}}"#;
		let event = scheduled_event(raw.as_bytes()).unwrap();
		assert_eq!(event.entity_type, model::ScheduledEventEntity::External);
		assert_eq!(event.location.as_deref(), Some("Town hall"));
		assert_eq!(event.host_channel(), None);
	}

	#[test]
	fn unknown_status_or_entity_type_is_refused() {
		for field in ["status", "entity_type"] {
			let raw = EVENT.replace(&format!("\"{field}\":"), &format!("\"{field}\":9,"));
			assert!(
				scheduled_event(raw.as_bytes()).is_err(),
				"an unknown {field} must be refused"
			);
		}
	}

	#[test]
	fn oversized_text_is_refused_while_parsing() {
		let long = "x".repeat(model::MAX_EVENT_NAME_BYTES + 1);
		let raw = EVENT.replace("\"Release\"", &format!("\"{long}\""));
		assert!(scheduled_event(raw.as_bytes()).is_err());
	}

	#[test]
	fn a_stage_instance_decodes_into_the_model() {
		let raw = r#"{"id":"3","guild_id":"2","topic":"Keynote","privacy_level":1,"discoverable_disabled":false}"#;
		let stage = stage_instance(raw.as_bytes()).unwrap();
		assert_eq!(stage.id, Id(3));
		assert_eq!(stage.topic, "Keynote");
		assert_eq!(stage.privacy_level, model::PRIVACY_PUBLIC);
	}

	#[test]
	fn event_lists_are_bounded_by_count() {
		let many: Vec<serde_json::Value> = (0..model::MAX_SCHEDULED_EVENTS as u64 + 1)
			.map(|index| {
				serde_json::json!({
					"id": index + 1,
					"guild_id": 2,
					"name": "Event",
					"privacy_level": 2,
					"status": 1,
					"entity_type": 3,
				})
			})
			.collect();
		let bytes = serde_json::to_vec(&many).unwrap();
		assert!(scheduled_events(&bytes).is_err());
	}
}
