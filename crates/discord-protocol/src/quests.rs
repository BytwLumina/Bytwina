//! Discord Quests.
//!
//! The wire shape follows the userdoccers documentation
//! (`docs.discord.food/resources/quests`); Discord has no public developer documentation for
//! it. Every list and map is bounded while it is parsed, so a hostile payload never grows past
//! the model's ceilings.

use model::Id;
use serde::Deserialize;

/// Rewards read from the configuration; only the first name is kept.
const MAX_REWARDS: usize = 8;
/// Platforms read from the configuration; only the first is kept.
const MAX_PLATFORMS: usize = 8;

fn bounded_text<'de, D: serde::Deserializer<'de>, const MAX: usize>(
	d: D,
) -> Result<String, D::Error> {
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
			bounded_text::<D, MAX>(d).map(Some)
		}
	}
	d.deserialize_option(Text::<MAX>)
}

fn text_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
	bounded_text::<D, { model::quests::MAX_QUEST_TEXT_BYTES }>(d)
}

fn optional_text_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	optional_text::<D, { model::quests::MAX_QUEST_TEXT_BYTES }>(d)
}

fn task_type_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
	bounded_text::<D, { model::quests::MAX_TASK_TYPE_BYTES }>(d)
}

fn asset_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	optional_text::<D, { model::quests::MAX_QUEST_ASSET_BYTES }>(d)
}

fn color_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	optional_text::<D, 16>(d)
}

/// Discord sends RFC 3339 timestamps; the model keeps Unix seconds.
fn seconds(timestamp: Option<crate::Timestamp>) -> Option<i64> {
	timestamp
		.and_then(|value| value.unix_ms())
		.map(|millis| millis.div_euclid(1000))
}

/// Parses a `#RRGGBB` gradient stop.
pub fn hex_color(value: &str) -> Option<u32> {
	let hex = value.strip_prefix('#').unwrap_or(value);
	if hex.len() != 6 || !hex.is_ascii() {
		return None;
	}
	u32::from_str_radix(hex, 16).ok()
}

#[derive(Deserialize)]
pub struct ApplicationDto {
	#[serde(default)]
	pub id: Option<Id>,
}

#[derive(Deserialize)]
pub struct AssetsDto {
	#[serde(default, deserialize_with = "asset_field")]
	pub hero: Option<String>,
}

#[derive(Deserialize)]
pub struct ColorsDto {
	#[serde(default, deserialize_with = "color_field")]
	pub primary: Option<String>,
	#[serde(default, deserialize_with = "color_field")]
	pub secondary: Option<String>,
}

#[derive(Deserialize)]
pub struct MessagesDto {
	#[serde(deserialize_with = "text_field")]
	pub quest_name: String,
	#[serde(default, deserialize_with = "optional_text_field")]
	pub game_title: Option<String>,
	#[serde(default, deserialize_with = "optional_text_field")]
	pub game_publisher: Option<String>,
}

#[derive(Deserialize)]
pub struct TaskDto {
	#[serde(rename = "type", deserialize_with = "task_type_field")]
	pub kind: String,
	#[serde(default)]
	pub target: u64,
}

/// The task map, bounded by count and by key length while it is read.
#[derive(Default)]
pub struct TaskMap(pub Vec<TaskDto>);
impl<'de> serde::Deserialize<'de> for TaskMap {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct Map;
		impl<'de> serde::de::Visitor<'de> for Map {
			type Value = TaskMap;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a bounded quest task map")
			}
			fn visit_map<A: serde::de::MapAccess<'de>>(
				self,
				mut map: A,
			) -> Result<Self::Value, A::Error> {
				let mut tasks = Vec::new();
				while let Some(key) = map.next_key::<String>()? {
					if key.len() > model::quests::MAX_TASK_TYPE_BYTES {
						return Err(serde::de::Error::custom("Quest task type capacity"));
					}
					if tasks.len() >= model::quests::MAX_TASKS {
						return Err(serde::de::Error::custom("Too many quest tasks"));
					}
					tasks.push(map.next_value::<TaskDto>()?);
				}
				Ok(TaskMap(tasks))
			}
		}
		d.deserialize_map(Map)
	}
}

#[derive(Deserialize)]
pub struct TaskConfigDto {
	#[serde(default)]
	pub tasks: TaskMap,
}

#[derive(Deserialize)]
pub struct RewardMessagesDto {
	#[serde(default, deserialize_with = "optional_text_field")]
	pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct RewardDto {
	#[serde(default)]
	pub messages: Option<RewardMessagesDto>,
}

/// Reads the reward list but keeps only the first reward's name.
fn first_reward<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
	struct List;
	impl<'de> serde::de::Visitor<'de> for List {
		type Value = Option<String>;
		fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
			f.write_str("a bounded quest reward list")
		}
		fn visit_seq<A: serde::de::SeqAccess<'de>>(
			self,
			mut seq: A,
		) -> Result<Self::Value, A::Error> {
			let mut name = None;
			let mut index = 0usize;
			while let Some(reward) = seq.next_element::<RewardDto>()? {
				if index == 0 {
					name = reward
						.messages
						.and_then(|messages| messages.name)
						.filter(|name| !name.is_empty());
				}
				index += 1;
				if index > MAX_REWARDS {
					return Err(serde::de::Error::custom("Too many quest rewards"));
				}
			}
			Ok(name)
		}
	}
	d.deserialize_seq(List)
}

/// Reads the platform list but keeps only the first entry.
fn first_platform<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u8>, D::Error> {
	struct List;
	impl<'de> serde::de::Visitor<'de> for List {
		type Value = Option<u8>;
		fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
			f.write_str("a bounded quest platform list")
		}
		fn visit_seq<A: serde::de::SeqAccess<'de>>(
			self,
			mut seq: A,
		) -> Result<Self::Value, A::Error> {
			let mut platform = None;
			let mut index = 0usize;
			while let Some(value) = seq.next_element::<u8>()? {
				if index == 0 {
					platform = Some(value);
				}
				index += 1;
				if index > MAX_PLATFORMS {
					return Err(serde::de::Error::custom("Too many quest platforms"));
				}
			}
			Ok(platform)
		}
	}
	d.deserialize_seq(List)
}

#[derive(Deserialize)]
pub struct RewardsConfigDto {
	#[serde(rename = "rewards", default, deserialize_with = "first_reward")]
	pub reward: Option<String>,
	#[serde(rename = "platforms", default, deserialize_with = "first_platform")]
	pub platform: Option<u8>,
}

#[derive(Deserialize)]
pub struct TaskProgressDto {
	#[serde(default)]
	pub value: u64,
	#[serde(default)]
	pub completed_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub heartbeat: Option<TaskHeartbeatDto>,
}

#[derive(Deserialize)]
pub struct TaskHeartbeatDto {
	#[serde(default)]
	pub last_beat_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub expires_at: Option<crate::Timestamp>,
}

/// The progress map of a user status, bounded while it is read.
#[derive(Default)]
pub struct ProgressMap(pub Vec<model::quests::TaskProgress>);
impl<'de> serde::Deserialize<'de> for ProgressMap {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct Map;
		impl<'de> serde::de::Visitor<'de> for Map {
			type Value = ProgressMap;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a bounded quest progress map")
			}
			fn visit_map<A: serde::de::MapAccess<'de>>(
				self,
				mut map: A,
			) -> Result<Self::Value, A::Error> {
				let mut progress = Vec::new();
				while let Some(key) = map.next_key::<String>()? {
					if key.len() > model::quests::MAX_TASK_TYPE_BYTES {
						return Err(serde::de::Error::custom("Quest task type capacity"));
					}
					if progress.len() >= model::quests::MAX_TASKS {
						return Err(serde::de::Error::custom("Too many quest progress rows"));
					}
					let value = map.next_value::<TaskProgressDto>()?;
					progress.push(model::quests::TaskProgress {
						kind: key,
						value: value.value,
						completed: value.completed_at.is_some(),
						heartbeat: value
							.heartbeat
							.map(|heartbeat| model::quests::TaskHeartbeat {
								last_beat_at: seconds(heartbeat.last_beat_at),
								expires_at: seconds(heartbeat.expires_at),
							}),
					});
				}
				Ok(ProgressMap(progress))
			}
		}
		d.deserialize_map(Map)
	}
}

#[derive(Deserialize)]
pub struct UserStatusDto {
	#[serde(default)]
	pub quest_id: Option<Id>,
	#[serde(default)]
	pub enrolled_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub completed_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub claimed_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub claimed_tier: Option<u32>,
	#[serde(default)]
	pub orb_quantity_claimed: Option<u64>,
	#[serde(default)]
	pub last_stream_heartbeat_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub stream_progress_seconds: u64,
	#[serde(default)]
	pub dismissed_quest_content: u32,
	#[serde(default)]
	pub progress: ProgressMap,
}
impl UserStatusDto {
	pub fn into_model(self) -> Result<model::quests::UserStatus, crate::DecodeError> {
		let status = self.status();
		if !status.valid() {
			return Err(crate::DecodeError);
		}
		Ok(status)
	}
	fn status(self) -> model::quests::UserStatus {
		let claimed_at = seconds(self.claimed_at);
		model::quests::UserStatus {
			quest: self.quest_id,
			enrolled_at: seconds(self.enrolled_at),
			// A claim always implies completion, even if the service omitted it.
			completed_at: seconds(self.completed_at).or(claimed_at),
			claimed_at,
			claimed_tier: self.claimed_tier,
			progress: self.progress.0,
			orb_quantity_claimed: self.orb_quantity_claimed,
			last_stream_heartbeat_at: seconds(self.last_stream_heartbeat_at),
			stream_progress_seconds: self.stream_progress_seconds,
			dismissed_quest_content: self.dismissed_quest_content,
		}
	}
}

#[derive(Deserialize)]
pub struct ConfigDto {
	pub id: Id,
	#[serde(default)]
	pub starts_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub expires_at: Option<crate::Timestamp>,
	#[serde(default)]
	pub application: Option<ApplicationDto>,
	#[serde(default)]
	pub assets: Option<AssetsDto>,
	#[serde(default)]
	pub colors: Option<ColorsDto>,
	pub messages: MessagesDto,
	#[serde(default)]
	pub task_config_v2: Option<TaskConfigDto>,
	#[serde(default)]
	pub rewards_config: Option<RewardsConfigDto>,
}

#[derive(Deserialize)]
pub struct QuestDto {
	pub id: Id,
	pub config: ConfigDto,
	#[serde(default)]
	pub user_status: Option<UserStatusDto>,
}
impl QuestDto {
	pub fn into_model(self) -> Result<model::quests::Quest, crate::DecodeError> {
		let status = self.user_status.map(UserStatusDto::status);
		let tasks = self
			.config
			.task_config_v2
			.map(|config| config.tasks.0)
			.unwrap_or_default()
			.into_iter()
			.map(|task| model::quests::Task {
				kind: task.kind,
				target: task.target,
				value: 0,
				completed: false,
			})
			.collect::<Vec<_>>();
		let rewards = self.config.rewards_config;
		let mut quest = model::quests::Quest {
			id: self.id,
			name: self.config.messages.quest_name,
			game: self.config.messages.game_title.unwrap_or_default(),
			publisher: self.config.messages.game_publisher.unwrap_or_default(),
			application: self
				.config
				.application
				.and_then(|application| application.id)
				.unwrap_or(Id(0)),
			starts_at: seconds(self.config.starts_at),
			expires_at: seconds(self.config.expires_at),
			hero: self.config.assets.and_then(|assets| assets.hero),
			primary: self
				.config
				.colors
				.as_ref()
				.and_then(|colors| colors.primary.as_deref())
				.and_then(hex_color),
			secondary: self
				.config
				.colors
				.as_ref()
				.and_then(|colors| colors.secondary.as_deref())
				.and_then(hex_color),
			tasks,
			reward: rewards.as_ref().and_then(|rewards| rewards.reward.clone()),
			platform: rewards
				.as_ref()
				.and_then(|rewards| rewards.platform)
				.filter(|platform| *platform <= model::quests::PLATFORM_MAX)
				.unwrap_or(model::quests::PLATFORM_CROSS),
			enrolled_at: None,
			completed_at: None,
			claimed_at: None,
		};
		if let Some(status) = status {
			status.apply_to(&mut quest);
		}
		if quest.id != self.config.id {
			return Err(crate::DecodeError);
		}
		if !quest.valid() {
			return Err(crate::DecodeError);
		}
		Ok(quest)
	}
}

/// The quest list, bounded while it is parsed.
pub struct Quests(pub Vec<model::quests::Quest>);
impl<'de> serde::Deserialize<'de> for Quests {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct List;
		impl<'de> serde::de::Visitor<'de> for List {
			type Value = Quests;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a bounded quest list")
			}
			fn visit_seq<A: serde::de::SeqAccess<'de>>(
				self,
				mut seq: A,
			) -> Result<Self::Value, A::Error> {
				let mut items: Vec<model::quests::Quest> = Vec::new();
				let mut total = 0usize;
				while let Some(dto) = seq.next_element::<QuestDto>()? {
					if items.len() >= model::quests::MAX_QUESTS {
						return Err(serde::de::Error::custom("Too many quests"));
					}
					let quest = dto
						.into_model()
						.map_err(|_| serde::de::Error::custom("Invalid quest"))?;
					total += quest.heap_bytes();
					if total > model::quests::MAX_QUEST_BYTES {
						return Err(serde::de::Error::custom("Quest capacity"));
					}
					items.push(quest);
				}
				Ok(Quests(items))
			}
		}
		d.deserialize_seq(List)
	}
}

#[derive(Deserialize)]
pub struct ListDto {
	pub quests: Quests,
	#[serde(default)]
	pub quest_enrollment_blocked_until: Option<crate::Timestamp>,
}

/// The current account's quests, as the discovery page reads them.
pub fn quest_list(bytes: &[u8]) -> Result<model::quests::QuestList, crate::DecodeError> {
	let dto = crate::decode::<ListDto>(bytes)?;
	let list = model::quests::QuestList {
		quests: dto.quests.0,
		blocked_until: seconds(dto.quest_enrollment_blocked_until),
	};
	if !model::quests::valid_quests(&list) {
		return Err(crate::DecodeError);
	}
	Ok(list)
}

/// One user status, as the gateway and the enrolment reply send it.
pub fn quest_status(bytes: &[u8]) -> Result<model::quests::UserStatus, crate::DecodeError> {
	crate::decode::<UserStatusDto>(bytes)?.into_model()
}

#[derive(Deserialize)]
pub struct ClaimDto {
	#[serde(default)]
	pub claimed_at: Option<crate::Timestamp>,
}

/// The claim reply: when the reward was granted.
pub fn quest_claim(bytes: &[u8]) -> Result<Option<i64>, crate::DecodeError> {
	let dto = crate::decode::<ClaimDto>(bytes)?;
	Ok(seconds(dto.claimed_at))
}

#[cfg(test)]
mod tests {
	use super::*;

	const QUEST: &str = r##"{
		"id": "8206816794116096000",
		"config": {
			"id": "8206816794116096000",
			"starts_at": "2025-02-21T18:00:00+00:00",
			"expires_at": "2025-02-28T01:00:00+00:00",
			"application": { "id": "891436233903964161", "name": "Cyberalien 2077" },
			"assets": { "hero": "hero.jpg", "game_tile": "gametile.jpg" },
			"colors": { "primary": "#E944D4", "secondary": "#5318A7" },
			"messages": {
				"quest_name": "Kill the Aliens",
				"game_title": "Cyberalien 2077",
				"game_publisher": "Alien Studios"
			},
			"task_config_v2": {
				"join_operator": "or",
				"tasks": {
					"PLAY_ON_DESKTOP": { "type": "PLAY_ON_DESKTOP", "target": 900 },
					"PLAY_ON_XBOX": { "type": "PLAY_ON_XBOX", "target": 900 }
				}
			},
			"rewards_config": {
				"rewards": [{ "type": 1, "sku_id": "1", "messages": { "name": "Cybernetic Headgear", "name_with_article": "a Cybernetic Headgear" } }],
				"platforms": [0]
			}
		},
		"user_status": {
			"user_id": "2",
			"quest_id": "8206816794116096000",
			"enrolled_at": "2025-02-22T10:00:00+00:00",
			"completed_at": null,
			"claimed_at": null,
			"progress": {
				"PLAY_ON_DESKTOP": { "value": 300, "event_name": "PLAY_ON_DESKTOP", "updated_at": "2025-02-22T10:05:00+00:00" }
			}
		},
		"preview": false
	}"##;

	#[test]
	fn a_quest_decodes_into_the_model() {
		let list = quest_list(
			format!(
				r#"{{"quests":[{QUEST}],"excluded_quests":[],"quest_enrollment_blocked_until":null}}"#
			)
			.as_bytes(),
		)
		.unwrap();
		assert_eq!(list.quests.len(), 1);
		let quest = &list.quests[0];
		assert_eq!(quest.name, "Kill the Aliens");
		assert_eq!(quest.game, "Cyberalien 2077");
		assert_eq!(quest.publisher, "Alien Studios");
		assert_eq!(quest.primary, Some(0xE944D4));
		assert_eq!(quest.tasks.len(), 2);
		assert_eq!(quest.tasks[0].value, 300);
		assert_eq!(quest.reward.as_deref(), Some("Cybernetic Headgear"));
		assert_eq!(quest.status(), model::quests::Status::Enrolled);
		assert_eq!(quest.expires_at, Some(1_740_704_400));
		assert_eq!(list.blocked_until, None);
	}

	#[test]
	fn a_completed_progress_row_marks_the_task() {
		let status = quest_status(
			br#"{"user_id":"2","quest_id":"9","enrolled_at":"2025-02-22T10:00:00+00:00","completed_at":"2025-02-22T11:00:00+00:00","claimed_at":null,"progress":{"PLAY_ON_DESKTOP":{"value":300,"completed_at":"2025-02-22T11:00:00+00:00"}}}"#,
		)
		.unwrap();
		assert!(status.progress[0].completed);
		assert_eq!(status.quest, Some(Id(9)));
		let mut quest = model::quests::Quest {
			id: Id(9),
			name: "Quest".to_owned(),
			game: String::new(),
			publisher: String::new(),
			application: Id(1),
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
			reward: None,
			platform: model::quests::PLATFORM_CROSS,
			enrolled_at: None,
			completed_at: None,
			claimed_at: None,
		};
		status.apply_to(&mut quest);
		assert_eq!(quest.status(), model::quests::Status::Completed);
		assert!(quest.tasks[0].completed);
	}

	#[test]
	fn a_claim_reply_reports_the_claim_time() {
		let claimed =
			quest_claim(br#"{"claimed_at":"2024-04-17T23:30:41.000321+00:00","entitlements":[]}"#)
				.unwrap();
		assert_eq!(claimed, Some(1_713_396_641));
		assert_eq!(quest_claim(b"{}").unwrap(), None);
	}

	#[test]
	fn oversized_text_and_bad_colours_are_handled() {
		assert_eq!(hex_color("#E944D4"), Some(0xE944D4));
		assert_eq!(hex_color("5318A7"), Some(0x5318A7));
		assert_eq!(hex_color("#GGGGGG"), None);
		assert_eq!(hex_color("#12345"), None);
		let long = "x".repeat(model::quests::MAX_QUEST_TEXT_BYTES + 1);
		let raw = QUEST.replace("\"Kill the Aliens\"", &format!("\"{long}\""));
		let wrapped = format!(r#"{{"quests":[{raw}]}}"#);
		assert!(quest_list(wrapped.as_bytes()).is_err());
	}

	#[test]
	fn a_list_beyond_the_count_ceiling_is_refused() {
		let entry = QUEST.replace("8206816794116096000", "8206816794116096001");
		let mut bodies = Vec::new();
		for index in 0..=model::quests::MAX_QUESTS as u64 {
			bodies.push(entry.replace("8206816794116096001", &(index + 1).to_string()));
		}
		let wrapped = format!(r#"{{"quests":[{}]}}"#, bodies.join(","));
		assert!(quest_list(wrapped.as_bytes()).is_err());
	}

	#[test]
	fn a_config_id_mismatch_is_refused() {
		// Change only the outer id, so the config keeps the original.
		let raw = QUEST.replacen("\"id\": \"8206816794116096000\",", "\"id\": \"1\",", 1);
		let wrapped = format!(r#"{{"quests":[{raw}]}}"#);
		assert!(quest_list(wrapped.as_bytes()).is_err());
	}
}
