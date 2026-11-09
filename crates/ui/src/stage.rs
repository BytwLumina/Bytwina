//! The guild events panel: a tab beside the channel list, a guild banner, and the
//! "Interested" action of one scheduled event.
//!
//! Everything here draws state that the client core owns; the panel only asks for a read and
//! sends the RSVP. No event data is cached in the view.

use crate::{design, i18n, icons, local_time};
use client_core::{Command, State};
use egui::RichText;
use model::Id;
use std::cell::Cell;

/// Which sidebar tab is on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
	/// The ordinary channel list.
	#[default]
	Channels,
	/// The guild's scheduled events.
	Events,
}

/// An action the user asked for; it is applied after the panel is drawn.
#[derive(Clone, Copy)]
enum Pending {
	/// A host moves a member.
	Moderate { user: Id, speaker: bool },
	/// This account raises or lowers the hand.
	Speak { asking: bool },
	/// This account subscribes to an event, or leaves it.
	Interest { event: Id, interested: bool },
	/// The subscriber list of one event is asked for.
	Subscribers { event: Id },
	/// A host starts a stage.
	StartStage { channel: Id },
	/// A host ends a stage.
	EndStage { channel: Id },
}

/// The events tab, the guild banner and the stage panel.
#[derive(Default)]
pub struct EventsUi {
	tab: Tab,
	/// The event whose subscriber list is on screen.
	subscribers: Option<Id>,
	/// The topic a host is typing for a new stage.
	stage_topic: String,
	/// The hand this account raised, before the gateway confirms it.
	pending_hand: Option<(Id, bool)>,
}

impl EventsUi {
	/// The tab on screen.
	pub fn tab(&self) -> Tab {
		self.tab
	}

	/// The tab row above the channel list.
	pub fn tabs(&mut self, ui: &mut egui::Ui, language: i18n::Language) {
		let colors = design::palette(ui);
		ui.horizontal(|ui| {
			ui.spacing_mut().item_spacing.x = 4.0;
			for (tab, label) in [
				(Tab::Channels, "events-tab-channels"),
				(Tab::Events, "events-tab-events"),
			] {
				let selected = self.tab == tab;
				let color = if selected {
					colors.text_strong
				} else {
					colors.muted
				};
				let response = ui.add_sized(
					[ui.available_width() / 2.0 - 2.0, 28.0],
					egui::Button::new(RichText::new(language.text(label)).color(color))
						.frame(false),
				);
				response.widget_info(|| {
					egui::WidgetInfo::selected(
						egui::Role::Button,
						ui.is_enabled(),
						selected,
						language.text(label),
					)
				});
				if response.clicked() {
					self.tab = tab;
				}
			}
		});
		ui.add_space(4.0);
	}

	/// The guild's next event, shown under the guild name.
	pub fn banner(&self, ui: &mut egui::Ui, state: &State, guild: Id, language: i18n::Language) {
		let Some(event) = state.next_guild_event(guild) else {
			return;
		};
		let colors = design::palette(ui);
		egui::Frame::new()
			.fill(colors.raised)
			.corner_radius(6)
			.inner_margin(egui::Margin::symmetric(8, 6))
			.show(ui, |ui| {
				ui.horizontal(|ui| {
					let (rect, _) =
						ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
					icons::paint(ui.painter(), icons::Icon::Calendar, rect, colors.accent);
					ui.add_space(6.0);
					ui.label(
						RichText::new(event.name.clone())
							.color(colors.text_strong)
							.strong(),
					);
				});
				if let Some(start) = event.scheduled_start_time
					&& let Some(when) = local_time::discord_timestamp(start, b'f')
				{
					ui.label(RichText::new(when).color(colors.muted).small());
				}
				ui.label(
					RichText::new(language.text("events-banner-open-events"))
						.color(colors.muted)
						.small(),
				);
			});
	}

	/// The guild's scheduled events, with the RSVP action of each one.
	pub fn panel(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		guild: Option<Id>,
		language: i18n::Language,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let Some(guild) = guild else {
			ui.label(RichText::new(language.text("events-no-guild")).color(colors.muted));
			return;
		};
		// The list is read once per visit; a guild that was never read is asked for here.
		if state.guild_events_status(guild) == client_core::stage::Status::Idle
			&& let Some(command) = state.request_scheduled_events(guild)
		{
			commands.push(command);
		}
		if let Some(error) = state.events.error(guild) {
			ui.label(RichText::new(crate::i18n::translate_if_key(error)).color(colors.warning));
			return;
		}
		// The events are read before they are drawn, so the rows are copied out first.
		let rows: Vec<(
			Id,
			bool,
			String,
			Option<String>,
			Option<String>,
			Option<i64>,
			u32,
			Option<String>,
		)> = state
			.guild_events(guild)
			.iter()
			.filter(|event| event.status.listed())
			.map(|event| {
				(
					event.id,
					event.interested,
					event.name.clone(),
					event.description.clone(),
					event.location.clone(),
					event.scheduled_start_time,
					event.user_count,
					event.cover.clone(),
				)
			})
			.collect();
		if rows.is_empty() {
			ui.label(RichText::new(language.text("events-empty")).color(colors.muted));
			return;
		}
		let pending = Cell::new(None);
		egui::ScrollArea::vertical()
			.id_salt("events-panel")
			.auto_shrink([false, false])
			.show(ui, |ui| {
				for (id, interested, name, description, location, start, going, cover) in rows {
					egui::Frame::new()
						.fill(colors.raised)
						.corner_radius(6)
						.inner_margin(egui::Margin::symmetric(8, 8))
						.show(ui, |ui| {
							ui.label(RichText::new(name).color(colors.text_strong).strong());
							if let Some(description) = description {
								ui.label(RichText::new(description).color(colors.text));
							}
							// The cover is a remote link, so only an https address is offered.
							if let Some(cover) = cover
								&& cover.starts_with("https://")
							{
								ui.hyperlink_to(
									RichText::new(language.text("events-cover"))
										.color(colors.accent),
									&cover,
								);
							}
							if let Some(location) = location {
								ui.label(RichText::new(location).color(colors.muted).small());
							}
							if let Some(start) = start
								&& let Some(when) = local_time::discord_timestamp(start, b'f')
							{
								ui.label(RichText::new(when).color(colors.muted).small());
							}
							ui.label(
								RichText::new(format!(
									"{} {}",
									going,
									language.text("events-going-label")
								))
								.color(colors.muted)
								.small(),
							);
							let label = if interested {
								language.text("events-interested")
							} else {
								language.text("events-interest")
							};
							if ui
								.add_sized(
									[ui.available_width(), 28.0],
									egui::Button::new(
										RichText::new(label).color(colors.text_strong),
									),
								)
								.clicked()
							{
								pending.set(Some(Pending::Interest {
									event: id,
									interested: !interested,
								}));
							}
							// The subscriber list is read on demand; the panel keeps it open
							// until the user closes it.
							let open = self.subscribers == Some(id);
							let list_label = if open {
								language.text("events-hide-subscribers")
							} else {
								language.text("events-show-subscribers")
							};
							if ui.button(list_label).clicked() {
								self.subscribers = if open { None } else { Some(id) };
								if !open {
									pending.set(Some(Pending::Subscribers { event: id }));
								}
							}
							if open {
								let members = state.event_subscribers(guild, id).to_vec();
								if members.is_empty() {
									ui.label(
										RichText::new(language.text("events-no-subscribers"))
											.color(colors.muted)
											.small(),
									);
								}
								for member in members {
									let name = crate::profiles::known_username(state, member)
										.unwrap_or_else(|| member.to_string());
									ui.label(RichText::new(name).color(colors.text).small());
								}
							}
						});
					ui.add_space(6.0);
				}
			});
		match pending.get() {
			Some(Pending::Interest { event, interested }) => {
				if let Some(command) = state.request_event_interest(guild, event, interested) {
					commands.push(command);
				}
			}
			Some(Pending::Subscribers { event }) => {
				if let Some(command) = state.request_event_subscribers(guild, event) {
					commands.push(command);
				}
			}
			_ => {}
		}
	}

	/// The live stage of one channel: its speakers, its audience and the host controls.
	pub fn stage_panel(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		channel: Id,
		guild: Option<Id>,
		language: i18n::Language,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let Some(guild) = guild else {
			return;
		};
		let instance = state
			.guild_stages(guild)
			.iter()
			.find(|stage| stage.id == channel)
			.map(|stage| stage.topic.clone());
		// The voice roster is the membership of the channel; the stage flags come from the
		// presence map the gateway keeps.
		let members: Vec<Id> = state
			.voice
			.roster
			.iter()
			.filter(|entry| entry.channel == channel)
			.map(|entry| entry.participant.user)
			.collect();
		let rows: Vec<(Id, String, bool, bool)> = members
			.into_iter()
			.map(|user| {
				// A member without a stage state sits in the audience.
				let presence = state.events.member_presence(channel, user).unwrap_or(
					client_core::stage::Presence {
						suppress: true,
						request_to_speak: false,
					},
				);
				(
					user,
					crate::profiles::known_username(state, user)
						.unwrap_or_else(|| user.to_string()),
					presence.suppress,
					presence.request_to_speak,
				)
			})
			.collect();
		let own = state.user.as_ref().map(|user| user.id);
		// Only a host may move another member; Discord asks for Mute Members there.
		let may_moderate =
			state.permission(channel, model::permissions::MUTE_MEMBERS) == Some(true);
		// Only a host may start or end the stage; Discord asks for Manage Channels there.
		let may_manage =
			state.permission(channel, model::permissions::MANAGE_CHANNELS) == Some(true);
		let pending = Cell::new(None);
		egui::Frame::new()
			.fill(colors.raised)
			.corner_radius(8)
			.inner_margin(egui::Margin::symmetric(12, 10))
			.show(ui, |ui| {
				match &instance {
					Some(topic) => {
						ui.label(
							RichText::new(topic.clone())
								.color(colors.text_strong)
								.strong(),
						);
					}
					None => {
						ui.label(
							RichText::new(language.text("events-stage-not-live"))
								.color(colors.muted),
						);
					}
				}
				let mut speakers = Vec::new();
				let mut audience = Vec::new();
				for (user, name, suppress, asking) in rows {
					if suppress {
						audience.push((user, name, asking));
					} else {
						speakers.push((user, name, asking));
					}
				}
				ui.label(
					RichText::new(language.text("events-stage-speakers")).color(colors.text_strong),
				);
				if speakers.is_empty() {
					ui.label(
						RichText::new(language.text("events-stage-no-speakers"))
							.color(colors.muted),
					);
				}
				for (user, name, asking) in speakers {
					ui.horizontal(|ui| {
						ui.label(RichText::new(name).color(colors.text));
						if asking {
							ui.label(
								RichText::new(language.text("events-stage-asked-to-speak"))
									.color(colors.accent)
									.small(),
							);
						}
						if may_moderate && own != Some(user) {
							let label = language.text("events-stage-move-to-audience");
							if ui.button(label).clicked() {
								pending.set(Some(Pending::Moderate {
									user,
									speaker: false,
								}));
							}
						}
					});
				}
				ui.label(
					RichText::new(language.text("events-stage-audience")).color(colors.text_strong),
				);
				if audience.is_empty() {
					ui.label(
						RichText::new(language.text("events-stage-no-audience"))
							.color(colors.muted),
					);
				}
				for (user, name, asking) in audience {
					ui.horizontal(|ui| {
						ui.label(RichText::new(name).color(colors.text));
						if asking {
							ui.label(
								RichText::new(language.text("events-stage-asked-to-speak"))
									.color(colors.accent)
									.small(),
							);
						}
						if may_moderate && own != Some(user) {
							let label = language.text("events-stage-make-speaker");
							if ui.button(label).clicked() {
								pending.set(Some(Pending::Moderate {
									user,
									speaker: true,
								}));
							}
						}
					});
				}
				// The hand belongs to this account only. The button shows the pending state
				// right away; the gateway confirms it a moment later.
				if let Some(own) = own {
					let asking = self
						.pending_hand
						.filter(|(where_, _)| *where_ == channel)
						.map(|(_, asking)| asking)
						.unwrap_or_else(|| {
							state
								.events
								.member_presence(channel, own)
								.is_some_and(|presence| presence.request_to_speak)
						});
					let label = if asking {
						language.text("events-stage-lower-hand")
					} else {
						language.text("events-stage-raise-hand")
					};
					if ui
						.add_sized([ui.available_width(), 30.0], egui::Button::new(label))
						.clicked()
					{
						self.pending_hand = Some((channel, !asking));
						pending.set(Some(Pending::Speak { asking: !asking }));
					}
				}
				// The stage itself is a host's tool.
				if may_manage {
					if instance.is_some() {
						if ui
							.add_sized(
								[ui.available_width(), 30.0],
								egui::Button::new(language.text("events-stage-end")),
							)
							.clicked()
						{
							pending.set(Some(Pending::EndStage { channel }));
						}
					} else {
						ui.label(
							RichText::new(language.text("events-stage-topic")).color(colors.muted),
						);
						ui.text_edit_singleline(&mut self.stage_topic);
						if ui
							.add_sized(
								[ui.available_width(), 30.0],
								egui::Button::new(language.text("events-stage-start")),
							)
							.clicked()
						{
							pending.set(Some(Pending::StartStage { channel }));
						}
					}
				}
			});
		match pending.get() {
			Some(Pending::Moderate { user, speaker }) => {
				if let Some(command) = state.request_stage_moderate(guild, channel, user, speaker) {
					commands.push(command);
				}
			}
			Some(Pending::Speak { asking }) => {
				if let Some(command) = state.request_stage_speak(guild, channel, asking) {
					commands.push(command);
				}
			}
			Some(Pending::StartStage { channel }) => {
				if let Some(command) =
					state.request_stage_instance(guild, channel, &self.stage_topic)
				{
					commands.push(command);
				}
			}
			Some(Pending::EndStage { channel }) => {
				if let Some(command) = state.request_stage_instance_end(guild, channel) {
					commands.push(command);
				}
			}
			_ => {}
		}
	}
}
