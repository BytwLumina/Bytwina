//! Server discovery browsing. Community-documented user routes; ordinary results are
//! verified and partnered servers only.
use crate::{DiscordApi, Failure};
use client_core::{
	Event,
	discovery::{Command, Event as DiscoveryEvent},
};
use discord_protocol::discovery;
use model::discovery::PAGE_SIZE;
use reqwest::Method;

impl DiscordApi {
	pub(super) async fn discovery_command(&self, command: Command) -> Event {
		Event::Discovery(match command {
			Command::Load {
				category,
				offset,
				request,
			} => DiscoveryEvent::Page {
				request,
				result: self
					.discoverable_guilds(category, offset)
					.await
					.map_err(|failure| failure.label()),
			},
		})
	}

	async fn discoverable_guilds(
		&self,
		category: Option<u32>,
		offset: u32,
	) -> Result<model::discovery::Page, Failure> {
		if offset % PAGE_SIZE != 0 || offset > 0xFFFF {
			return Err(Failure::Protocol);
		}
		let categories = self.discovery_categories().await.unwrap_or_default();
		let mut path = format!("/discoverable-guilds?limit={PAGE_SIZE}&offset={offset}");
		if let Some(category) = category {
			path.push_str(&format!("&categories={category}"));
		}
		let bytes = self
			.request_limited(Method::GET, &path, None, discovery::MAX_DISCOVERY_WIRE)
			.await?;
		discovery::decode_page(&bytes, categories).map_err(|_| Failure::Protocol)
	}

	/// The category rail. A failure here does not fail the page; it only hides the rail.
	async fn discovery_categories(&self) -> Result<Vec<model::discovery::Category>, Failure> {
		let bytes = self
			.request_limited(
				Method::GET,
				"/discovery/categories?primary_only=true",
				None,
				discovery::MAX_CATEGORIES_WIRE,
			)
			.await?;
		discovery::decode_categories(&bytes).map_err(|_| Failure::Protocol)
	}
}
