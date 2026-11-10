//! Soundboard listing and playback. Unofficial normal-user routes; see docs/discord-compatibility.md.
use crate::{DiscordApi, Failure};
use discord_protocol::soundboard;
use model::{Id, SoundboardSound};
use reqwest::Method;

impl DiscordApi {
	/// Sounds every account may use, plus the ones owned by this guild.
	pub(super) async fn load_soundboard(
		&self,
		guild: Option<Id>,
	) -> Result<Vec<SoundboardSound>, Failure> {
		let mut sounds = Vec::new();
		let defaults = self
			.request_limited(
				Method::GET,
				"/soundboard-default-sounds",
				None,
				soundboard::MAX_SOUNDBOARD_WIRE,
			)
			.await?;
		sounds.extend(soundboard::decode_list(&defaults, None).map_err(|_| Failure::Protocol)?);
		if let Some(guild) = guild {
			if guild.0 == 0 {
				return Err(Failure::Protocol);
			}
			let path = format!("/guilds/{guild}/soundboard-sounds");
			let bytes = self
				.request_limited(Method::GET, &path, None, soundboard::MAX_SOUNDBOARD_WIRE)
				.await?;
			sounds.extend(
				soundboard::decode_guild_list(&bytes, guild).map_err(|_| Failure::Protocol)?,
			);
		}
		Ok(sounds)
	}

	/// Play one sound in the voice channel the owner is connected to. The service rejects this
	/// while server muted, deafened or suppressed, so the caller gates on local voice state.
	pub(super) async fn send_soundboard_sound(
		&self,
		channel: Id,
		sound: Id,
		source_guild: Option<Id>,
	) -> Result<(), Failure> {
		if channel.0 == 0 || sound.0 == 0 || source_guild.is_some_and(|id| id.0 == 0) {
			return Err(Failure::Protocol);
		}
		let path = format!("/channels/{channel}/send-soundboard-sound");
		let body = serde_json::json!({
			"sound_id": sound.to_string(),
			"source_guild_id": source_guild.map(|guild| guild.to_string()),
		});
		self.request_limited(
			Method::POST,
			&path,
			Some(body),
			soundboard::MAX_SOUNDBOARD_WIRE,
		)
		.await?;
		Ok(())
	}
}
