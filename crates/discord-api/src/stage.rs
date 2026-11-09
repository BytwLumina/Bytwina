//! Guild scheduled events over REST.
//!
//! The routes are documented at `docs.discord.food/resources/guild-scheduled-event`: reading a
//! guild's events, reading one event's subscribers, and setting this account's own interest.
//! Everything is session memory; nothing here is persisted.

use crate::{DiscordApi, Failure};
use model::Id;
use reqwest::Method;

/// A guild's event list is a short list; the cap keeps a hostile reply bounded.
const MAX_EVENT_WIRE: usize = 512 * 1024;
/// One event's subscriber list is a page of a hundred members.
const MAX_SUBSCRIBER_WIRE: usize = 64 * 1024;
/// The interest reply is an empty body.
const MAX_REPLY_WIRE: usize = 4 * 1024;
/// One stage instance is a small object.
const MAX_STAGE_WIRE: usize = 16 * 1024;

impl DiscordApi {
	/// The guild's scheduled events.
	pub(super) async fn scheduled_events(
		&self,
		guild: Id,
	) -> Result<Vec<model::ScheduledEvent>, Failure> {
		if guild.0 == 0 {
			return Err(Failure::Protocol);
		}

		let bytes = self
			.request_limited(
				Method::GET,
				&format!("/guilds/{guild}/scheduled-events"),
				None,
				MAX_EVENT_WIRE,
			)
			.await?;

		discord_protocol::stage::scheduled_events(&bytes).map_err(|_| Failure::Protocol)
	}

	/// The members subscribed to one event.
	pub(super) async fn event_subscribers(&self, guild: Id, event: Id) -> Result<Vec<Id>, Failure> {
		if guild.0 == 0 || event.0 == 0 {
			return Err(Failure::Protocol);
		}

		let bytes = self
			.request_limited(
				Method::GET,
				&format!("/guilds/{guild}/scheduled-events/{event}/users"),
				None,
				MAX_SUBSCRIBER_WIRE,
			)
			.await?;

		discord_protocol::stage::event_subscribers(&bytes).map_err(|_| Failure::Protocol)
	}

	/// Sets or clears this account's interest in one event.
	pub(super) async fn set_event_interest(
		&self,
		guild: Id,
		event: Id,
		interested: bool,
	) -> Result<(), Failure> {
		if guild.0 == 0 || event.0 == 0 {
			return Err(Failure::Protocol);
		}

		self.request_limited(
			Method::PUT,
			&format!("/guilds/{guild}/scheduled-events/{event}/users/@me"),
			Some(serde_json::json!({ "interested": interested })),
			MAX_REPLY_WIRE,
		)
		.await
		.map(|_| ())
	}

	/// Moves one member of a voice channel, or asks to speak on a stage.
	///
	/// The route is documented at `docs.discord.food/resources/voice`: a host moves a member with
	/// `suppress`, and a member raises the hand with `request_to_speak_timestamp`.
	pub(super) async fn set_voice_state(
		&self,
		guild: Id,
		user: Id,
		body: serde_json::Value,
	) -> Result<(), Failure> {
		if guild.0 == 0 || user.0 == 0 {
			return Err(Failure::Protocol);
		}

		self.request_limited(
			Method::PATCH,
			&format!("/guilds/{guild}/voice-states/{user}"),
			Some(body),
			MAX_REPLY_WIRE,
		)
		.await
		.map(|_| ())
	}

	/// The current time in the shape Discord's voice-state timestamp expects.
	fn now_timestamp() -> Option<String> {
		time::OffsetDateTime::now_utc()
			.format(&time::format_description::well_known::Rfc3339)
			.ok()
	}

	/// Asks to speak in a stage channel, or takes the hand back.
	pub(super) async fn request_to_speak(
		&self,
		guild: Id,
		channel: Id,
		user: Id,
		asking: bool,
	) -> Result<(), Failure> {
		let timestamp = if asking {
			match Self::now_timestamp() {
				Some(timestamp) => serde_json::Value::String(timestamp),
				None => return Err(Failure::Protocol),
			}
		} else {
			serde_json::Value::Null
		};
		let body = serde_json::json!({
			"channel_id": channel.to_string(),
			"request_to_speak_timestamp": timestamp,
		});
		self.set_voice_state(guild, user, body).await
	}

	/// A host moves one member between the stage and the audience.
	pub(super) async fn moderate_stage(
		&self,
		guild: Id,
		channel: Id,
		user: Id,
		speaker: bool,
	) -> Result<(), Failure> {
		let body = serde_json::json!({
			"channel_id": channel.to_string(),
			"suppress": !speaker,
		});
		self.set_voice_state(guild, user, body).await
	}

	/// Starts a stage in one stage channel.
	///
	/// The route is documented at `docs.discord.food/resources/stage-instance`; it needs the
	/// Manage Channels permission, which the client core checks before sending it.
	pub(super) async fn create_stage_instance(
		&self,
		channel: Id,
		topic: &str,
	) -> Result<model::StageInstance, Failure> {
		if channel.0 == 0 {
			return Err(Failure::Protocol);
		}
		let topic = topic.trim();
		if topic.is_empty() || topic.len() > model::MAX_STAGE_TOPIC_BYTES {
			return Err(Failure::Protocol);
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/channels/{channel}/stage-instances"),
				Some(serde_json::json!({
					"topic": topic,
					"privacy_level": model::PRIVACY_GUILD_ONLY,
				})),
				MAX_STAGE_WIRE,
			)
			.await?;
		discord_protocol::stage::stage_instance(&bytes).map_err(|_| Failure::Protocol)
	}

	/// Ends the stage of one stage channel.
	pub(super) async fn delete_stage_instance(&self, channel: Id) -> Result<(), Failure> {
		if channel.0 == 0 {
			return Err(Failure::Protocol);
		}
		self.request_limited(
			Method::DELETE,
			&format!("/channels/{channel}/stage-instances"),
			None,
			MAX_REPLY_WIRE,
		)
		.await
		.map(|_| ())
	}
}

/// The i18n key for one failure. Only fixed local keys are used; a remote value never
/// reaches the panel.
pub(super) fn failure_key(failure: Failure) -> &'static str {
	match failure {
		Failure::Expired => "events-panel-expired",
		Failure::Challenged => "events-panel-challenged",
		Failure::Forbidden => "events-panel-forbidden",
		Failure::RateLimited => "events-panel-rate-limited",
		Failure::Network => "events-panel-network",
		Failure::Ambiguous => "events-panel-unknown",
		Failure::Protocol | Failure::ProtocolAt(_) => "events-panel-unavailable",
		Failure::Capacity | Failure::CapacityAt(_) => "events-panel-unavailable",
		Failure::InvalidCredential => "events-panel-unavailable",
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use client_core::{Command, Event};
	use std::{sync::Arc, time::Duration};
	use tokio::{
		io::{AsyncReadExt, AsyncWriteExt},
		net::TcpListener,
	};

	async fn create_api(listener: &TcpListener) -> DiscordApi {
		let mut api = DiscordApi::new(Arc::new(
			client_core::auth::SessionSecret::from_owner_input("SYNTHETIC_EVENT_TOKEN".into())
				.unwrap(),
		))
		.unwrap();

		api.base = format!("http://{}", listener.local_addr().unwrap());

		api
	}

	async fn respond(listener: &TcpListener, route: &str, status: &str, body: &str) {
		let (mut socket, _) = listener.accept().await.unwrap();

		let mut request = Vec::new();

		loop {
			let mut buffer = [0; 1024];
			let n = socket.read(&mut buffer).await.unwrap();

			assert!(n > 0);

			request.extend_from_slice(&buffer[..n]);

			assert!(request.len() < 4096);

			if request.windows(4).any(|window| window == b"\r\n\r\n") {
				break;
			}
		}

		let text = std::str::from_utf8(&request).unwrap();

		assert!(text.starts_with(&format!("GET {route} HTTP/1.1\r\n")));
		assert!(text.contains("SYNTHETIC_EVENT_TOKEN"));

		socket
			.write_all(
				format!(
					"HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
					body.len()
				)
				.as_bytes(),
			)
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn a_guild_event_list_is_read() {
		tokio::time::timeout(Duration::from_secs(10), async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let api = create_api(&listener).await;

			let body = r#"[{"id":"9","guild_id":"1","name":"Release","privacy_level":2,"status":1,"entity_type":3}]"#;

			let server = tokio::spawn(async move {
				respond(&listener, "/guilds/1/scheduled-events", "200 OK", body).await;
			});

			let Event::ScheduledEvent(client_core::stage::Event::Snapshot {
				guild,
				events,
				..
			}) = api
				.execute(Command::ScheduledEvents {
					guild: Id(1),
					request: 1,
				})
				.await
			else {
				panic!()
			};

			assert_eq!(guild, Id(1));
			assert_eq!(events.len(), 1);
			assert_eq!(events[0].name, "Release");

			server.await.unwrap();
		})
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn a_read_without_a_guild_is_refused() {
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let api = create_api(&listener).await;

		assert!(matches!(
			api.scheduled_events(Id(0)).await,
			Err(Failure::Protocol)
		));
		assert!(matches!(
			api.event_subscribers(Id(1), Id(0)).await,
			Err(Failure::Protocol)
		));
		assert!(matches!(
			api.set_event_interest(Id(1), Id(0), true).await,
			Err(Failure::Protocol)
		));
	}

	#[tokio::test]
	async fn interest_is_sent_as_a_documented_put() {
		tokio::time::timeout(Duration::from_secs(10), async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let api = create_api(&listener).await;

			let server = tokio::spawn(async move {
				let (mut socket, _) = listener.accept().await.unwrap();
				let mut request = Vec::new();
				loop {
					let mut buffer = [0; 1024];
					let n = socket.read(&mut buffer).await.unwrap();
					assert!(n > 0);
					request.extend_from_slice(&buffer[..n]);
					assert!(request.len() < 4096);
					if request.windows(4).any(|window| window == b"\r\n\r\n") {
						break;
					}
				}
				let text = std::str::from_utf8(&request).unwrap();
				assert!(
					text.starts_with("PUT /guilds/1/scheduled-events/9/users/@me HTTP/1.1\r\n"),
					"{text}"
				);
				assert!(text.contains(r#"{"interested":true}"#), "{text}");
				socket
					.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
					.await
					.unwrap();
			});

			api.set_event_interest(Id(1), Id(9), true).await.unwrap();

			server.await.unwrap();
		})
		.await
		.unwrap();
	}
}
