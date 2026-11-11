//! Saved messages and reminders. Unofficial normal-user routes; see docs/discord-compatibility.md.
use crate::{DiscordApi, Failure};
use client_core::{
	Event,
	saved_messages::{Command, Event as SavedEvent},
};
use discord_protocol::saved_messages;
use model::Id;
use reqwest::Method;

/// RFC 3339 only: the service rejects local times, and a past date is still a valid reminder.
fn due(due_at: Option<i64>) -> Result<Option<String>, Failure> {
	let Some(due_at) = due_at else {
		return Ok(None);
	};
	if !model::saved_messages::valid_timestamp(due_at) {
		return Err(Failure::Protocol);
	}
	let time = time::OffsetDateTime::from_unix_timestamp(due_at).map_err(|_| Failure::Protocol)?;
	time.format(&time::format_description::well_known::Rfc3339)
		.map(Some)
		.map_err(|_| Failure::Protocol)
}

impl DiscordApi {
	pub(super) async fn saved_messages_command(&self, command: Command) -> Event {
		Event::SavedMessages(match command {
			Command::Load { request } => SavedEvent::List {
				request,
				result: self.saved_message_list().await,
			},
			Command::Save {
				channel,
				message,
				due_at,
				request,
			} => SavedEvent::Saved {
				channel,
				message,
				request,
				result: self.save_message(channel, message, due_at).await,
			},
			Command::Remove {
				channel,
				message,
				request,
			} => SavedEvent::Removed {
				request,
				result: self
					.remove_saved_message(channel, message)
					.await
					.map(|(found, _): (Id, _)| (found, message)),
			},
		})
	}

	async fn saved_message_list(
		&self,
	) -> Result<Vec<model::saved_messages::SavedMessage>, Failure> {
		let bytes = self
			.request_limited(
				Method::GET,
				"/users/@me/saved-messages",
				None,
				saved_messages::MAX_SAVED_MESSAGES_WIRE,
			)
			.await?;
		saved_messages::decode_list(&bytes).map_err(|_| Failure::Protocol)
	}

	/// One explicit write, without retries. The caller never has to guess whether it landed.
	async fn save_message(
		&self,
		channel: Id,
		message: Id,
		due_at: Option<i64>,
	) -> Result<model::saved_messages::SavedMessage, Failure> {
		if channel.0 == 0 || message.0 == 0 {
			return Err(Failure::Protocol);
		}
		let path = format!("/users/@me/saved-messages/{channel}/{message}");
		let body = serde_json::json!({ "due_at": due(due_at)? });
		let bytes = self
			.request_limited(
				Method::PUT,
				&path,
				Some(body),
				saved_messages::MAX_SAVED_MESSAGES_WIRE,
			)
			.await?;
		let row = saved_messages::decode_saved_message(&bytes).map_err(|_| Failure::Protocol)?;
		if row.key() != (channel, message) {
			return Err(Failure::ProtocolAt("saved-messages-answer-mismatch"));
		}
		Ok(row)
	}

	async fn remove_saved_message(&self, channel: Id, message: Id) -> Result<(Id, ()), Failure> {
		if channel.0 == 0 || message.0 == 0 {
			return Err(Failure::Protocol);
		}
		let path = format!("/users/@me/saved-messages/{channel}/{message}");
		self.request_limited(Method::DELETE, &path, None, 0)
			.await
			.map(|_| (channel, ()))
	}
}
