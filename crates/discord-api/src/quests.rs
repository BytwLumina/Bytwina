//! Discord Quests over REST.
//!
//! The routes are documented only by the userdoccers project
//! (`docs.discord.food/resources/quests`), so treat them as restricted and unverified. The
//! client only reads the account's own list, accepts one quest and claims a completed reward.

use crate::{DiscordApi, Failure};
use model::Id;
use reqwest::Method;

/// The discovery list is bounded by the model's ceilings; the wire cap keeps a hostile reply
/// from growing past them while it is read.
const MAX_QUEST_WIRE: usize = 1024 * 1024;
/// One user status or claim reply is a small object.
const MAX_REPLY_WIRE: usize = 16 * 1024;

impl DiscordApi {
	/// The account's current quests.
	pub(super) async fn quests(&self) -> Result<model::quests::QuestList, Failure> {
		let bytes = self
			.request_limited(Method::GET, "/quests/@me", None, MAX_QUEST_WIRE)
			.await?;
		discord_protocol::quests::quest_list(&bytes).map_err(|_| Failure::Protocol)
	}

	/// Accepts one quest and returns this account's new status.
	pub(super) async fn enroll_quest(
		&self,
		quest: Id,
	) -> Result<model::quests::UserStatus, Failure> {
		if quest.0 == 0 {
			return Err(Failure::Protocol);
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/quests/{quest}/enroll"),
				Some(serde_json::json!({
					"location": model::quests::CONTENT_QUEST_HOME_DESKTOP,
				})),
				MAX_REPLY_WIRE,
			)
			.await?;
		discord_protocol::quests::quest_status(&bytes).map_err(|_| Failure::Protocol)
	}

	pub(super) async fn quest_heartbeat(
		&self,
		quest: Id,
		application: Option<Id>,
		stream_key: Option<String>,
		terminal: bool,
	) -> Result<model::quests::UserStatus, Failure> {
		if quest.0 == 0
			|| application.is_some_and(|id| id.0 == 0)
			|| stream_key.as_ref().is_some_and(|key| key.len() > 256)
		{
			return Err(Failure::Protocol);
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/quests/{quest}/heartbeat"),
				Some(serde_json::json!({
					"application_id": application.map(|id| id.to_string()),
					"stream_key": stream_key,
					"terminal": terminal,
				})),
				MAX_REPLY_WIRE,
			)
			.await?;
		discord_protocol::quests::quest_status(&bytes).map_err(|_| Failure::Protocol)
	}

	pub(super) async fn dismiss_quest_content(
		&self,
		quest: Id,
		content: u32,
	) -> Result<model::quests::UserStatus, Failure> {
		if quest.0 == 0 || content == 0 || content > (1 << 31) {
			return Err(Failure::Protocol);
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/quests/{quest}/dismissible-content/{content}/dismiss"),
				None,
				MAX_REPLY_WIRE,
			)
			.await?;
		discord_protocol::quests::quest_status(&bytes).map_err(|_| Failure::Protocol)
	}

	/// Claims the reward of one completed quest; the reply is when it was granted.
	pub(super) async fn claim_quest(
		&self,
		quest: Id,
		platform: u8,
	) -> Result<Option<i64>, Failure> {
		if quest.0 == 0 || platform > model::quests::PLATFORM_MAX {
			return Err(Failure::Protocol);
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/quests/{quest}/claim-reward"),
				Some(serde_json::json!({
					"location": model::quests::CONTENT_QUEST_HOME_DESKTOP,
					"platform": platform,
				})),
				MAX_REPLY_WIRE,
			)
			.await?;
		discord_protocol::quests::quest_claim(&bytes).map_err(|_| Failure::Protocol)
	}
}

/// The i18n key for one failure. Only fixed local keys are used; a remote value never
/// reaches the panel.
pub(super) fn failure_key(failure: Failure) -> &'static str {
	match failure {
		Failure::Expired => "quests-panel-expired",
		Failure::Challenged => "quests-panel-challenged",
		Failure::Forbidden => "quests-panel-forbidden",
		Failure::RateLimited => "quests-panel-rate-limited",
		Failure::Network => "quests-panel-network",
		Failure::Ambiguous => "quests-panel-unknown",
		Failure::Protocol | Failure::ProtocolAt(_) => "quests-panel-unavailable",
		Failure::Capacity | Failure::CapacityAt(_) => "quests-panel-unavailable",
		Failure::InvalidCredential => "quests-panel-unavailable",
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
			client_core::auth::SessionSecret::from_owner_input("SYNTHETIC_QUEST_TOKEN".into())
				.unwrap(),
		))
		.unwrap();
		api.base = format!("http://{}", listener.local_addr().unwrap());
		api
	}

	async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
		let mut request = Vec::new();
		let header_end = loop {
			let mut buffer = [0; 1024];
			let n = socket.read(&mut buffer).await.unwrap();
			assert!(n > 0);
			request.extend_from_slice(&buffer[..n]);
			assert!(request.len() < 8192);
			if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
				break index + 4;
			}
		};
		let length = String::from_utf8_lossy(&request[..header_end])
			.lines()
			.find_map(|line| {
				let (name, value) = line.split_once(':')?;
				name.eq_ignore_ascii_case("content-length")
					.then(|| value.trim().parse::<usize>().ok())?
			})
			.unwrap_or(0);
		while request.len() < header_end + length {
			let mut buffer = [0; 1024];
			let n = socket.read(&mut buffer).await.unwrap();
			assert!(n > 0);
			request.extend_from_slice(&buffer[..n]);
		}
		String::from_utf8(request).unwrap()
	}

	#[tokio::test]
	async fn the_quest_list_is_read() {
		tokio::time::timeout(Duration::from_secs(10), async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let api = create_api(&listener).await;
			let body = r#"{"quests":[{"id":"9","config":{"id":"9","messages":{"quest_name":"Kill the Aliens","game_title":"Cyberalien 2077","game_publisher":"Alien Studios"},"task_config_v2":{"tasks":{"PLAY_ON_DESKTOP":{"type":"PLAY_ON_DESKTOP","target":900}}},"rewards_config":{"rewards":[{"type":1,"messages":{"name":"Cybernetic Headgear"}}],"platforms":[0]}},"user_status":null}]}"#;
			let server = tokio::spawn(async move {
				let (mut socket, _) = listener.accept().await.unwrap();
				let text = read_request(&mut socket).await;
				assert!(text.starts_with("GET /quests/@me HTTP/1.1\r\n"), "{text}");
				assert!(text.contains("SYNTHETIC_QUEST_TOKEN"));
				socket
					.write_all(
						format!(
							"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
							body.len()
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			});
			let Event::Quests(client_core::quests::Event::Snapshot { request, result }) = api
				.execute(Command::Quests { request: 1 })
				.await
			else {
				panic!()
			};
			assert_eq!(request, 1);
			let list = result.unwrap();
			assert_eq!(list.quests.len(), 1);
			assert_eq!(list.quests[0].name, "Kill the Aliens");
			assert_eq!(
				list.quests[0].status(),
				model::quests::Status::Available
			);
			server.await.unwrap();
		})
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn enrolling_posts_the_documented_body() {
		tokio::time::timeout(Duration::from_secs(10), async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let api = create_api(&listener).await;
			let body = r#"{"user_id":"2","quest_id":"9","enrolled_at":"2025-02-22T10:00:00+00:00","completed_at":null,"claimed_at":null,"progress":{}}"#;
			let server = tokio::spawn(async move {
				let (mut socket, _) = listener.accept().await.unwrap();
				let text = read_request(&mut socket).await;
				assert!(text.starts_with("POST /quests/9/enroll HTTP/1.1\r\n"), "{text}");
				assert!(text.contains(r#"{"location":11}"#), "{text}");
				socket
					.write_all(
						format!(
							"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
							body.len()
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			});
			let Event::Quests(client_core::quests::Event::Enrolled { quest, result, .. }) = api
				.execute(Command::EnrollQuest {
					quest: Id(9),
					request: 2,
				})
				.await
			else {
				panic!()
			};
			assert_eq!(quest, Id(9));
			assert!(result.unwrap().enrolled_at.is_some());
			server.await.unwrap();
		})
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn impossible_identifiers_are_refused_before_any_request() {
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let api = create_api(&listener).await;
		assert!(matches!(
			api.enroll_quest(Id(0)).await,
			Err(Failure::Protocol)
		));
		assert!(matches!(
			api.claim_quest(Id(9), 9).await,
			Err(Failure::Protocol)
		));
	}
}
