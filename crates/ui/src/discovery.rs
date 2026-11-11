//! Server discovery: one page of verified/partnered servers with a category rail.
//! Joining always goes through the existing invite preview, never a direct member write.
use crate::{MessagingUi, design, i18n};
use client_core::{Command, State, discovery::PAGE_SIZE};

#[derive(Clone)]
enum Action {
	Reload(Option<u32>, u32),
	Preview(String),
}

impl MessagingUi {
	pub(super) fn discovery_page(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let language = self.language;
		let mut action = None;
		if self.discovery_requested_generation != Some(state.generation) {
			self.discovery_requested_generation = Some(state.generation);
			self.discovery_requested = false;
		}
		// The first visit asks for a page; later visits reuse the one already loaded.
		if state.discovery.page().is_none()
			&& !state.discovery.loading()
			&& state.discovery.error().is_none()
			&& !self.discovery_requested
		{
			self.discovery_requested = true;
			action = Some(Action::Reload(None, 0));
		}
		egui::Frame::new()
			.inner_margin(egui::Margin::symmetric(24, 16))
			.show(ui, |ui| {
				ui.horizontal(|ui| {
					ui.label(
						design::semibold(ui, language.text("discovery-title"), 20.0)
							.color(colors.text_strong),
					);
					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						if ui.button(language.text("discovery-refresh")).clicked() {
							let offset = state.discovery.page().map_or(0, |page| page.offset);
							action = Some(Action::Reload(state.discovery.category(), offset));
						}
					});
				});
				ui.label(
					egui::RichText::new(language.text("discovery-description"))
						.color(colors.muted)
						.small(),
				);
				ui.add_space(8.0);
				let categories: Vec<_> = state
					.discovery
					.categories()
					.iter()
					.map(|category| (category.id, category.name.clone()))
					.collect();
				if !categories.is_empty() {
					ui.horizontal_wrapped(|ui| {
						if chip(
							ui,
							&language.text("discovery-all"),
							state.discovery.category().is_none(),
						) {
							action = Some(Action::Reload(None, 0));
						}
						for (id, name) in categories {
							if chip(ui, &name, state.discovery.category() == Some(id)) {
								action = Some(Action::Reload(Some(id), 0));
							}
						}
					});
					ui.add_space(8.0);
				}
				if let Some(error) = state.discovery.error() {
					ui.label(
						egui::RichText::new(i18n::translate_if_key(error)).color(colors.warning),
					);
					if ui.button(language.text("discovery-retry")).clicked() {
						action = Some(Action::Reload(state.discovery.category(), 0));
					}
				}
				ui.add(
					egui::TextEdit::singleline(&mut state.discovery.filter)
						.hint_text(language.text("discovery-filter"))
						.desired_width(f32::INFINITY),
				);
				ui.label(
					egui::RichText::new(language.text("discovery-filter-note"))
						.color(colors.muted)
						.small(),
				);
				ui.add_space(10.0);
				if state.discovery.loading() {
					ui.horizontal(|ui| {
						ui.spinner();
						ui.label(language.text("discovery-loading"));
					});
					return;
				}
				let guilds: Vec<_> = state.discovery.guilds().into_iter().cloned().collect();
				if guilds.is_empty() {
					ui.add_space(20.0);
					ui.label(
						egui::RichText::new(language.text("discovery-empty")).color(colors.muted),
					);
					return;
				}
				egui::ScrollArea::vertical()
					.id_salt("discovery-page")
					.auto_shrink([false, false])
					.show(ui, |ui| {
						for guild in &guilds {
							let joined = state.guild(guild.id).is_some();
							egui::Frame::new()
								.fill(colors.raised)
								.corner_radius(10)
								.inner_margin(egui::Margin::symmetric(14, 12))
								.show(ui, |ui| {
									ui.horizontal_top(|ui| {
										let key = guild
											.icon
											.as_deref()
											.filter(|hash| model::valid_avatar_hash(hash))
											.map(|hash| format!("guild-card-{}-{hash}", guild.id));
										self.avatars.show_icon(ui, key, 56.0, true, &guild.name);
										ui.vertical(|ui| {
											ui.horizontal(|ui| {
												ui.label(
													design::semibold(ui, guild.name.clone(), 15.0)
														.color(colors.text_strong),
												);
												if guild.verified || guild.partnered {
													ui.label(
														egui::RichText::new(language.text(
															if guild.partnered {
																"discovery-partnered"
															} else {
																"discovery-verified"
															},
														))
														.color(colors.accent)
														.small(),
													);
												}
											});
											let members =
												guild.approximate_member_count.to_string();
											let online =
												guild.approximate_presence_count.to_string();
											ui.label(
												egui::RichText::new(format!(
													"{} · {}",
													crate::i18n::translate_args(
														"discovery-members",
														&[("count", &members)]
													),
													crate::i18n::translate_args(
														"discovery-online",
														&[("count", &online)]
													)
												))
												.color(colors.muted)
												.small(),
											);
											if let Some(description) = &guild.description {
												ui.add(
													egui::Label::new(
														egui::RichText::new(description.clone())
															.color(colors.text)
															.small(),
													)
													.truncate(),
												);
											}
										});
										ui.with_layout(
											egui::Layout::right_to_left(egui::Align::TOP),
											|ui| {
												if joined {
													ui.label(
														egui::RichText::new(
															language.text("discovery-joined"),
														)
														.color(colors.muted),
													);
												} else if guild.vanity_url_code.is_some() {
													if ui
														.button(language.text("discovery-join"))
														.clicked()
													{
														action = Some(Action::Preview(
															guild
																.vanity_url_code
																.clone()
																.unwrap_or_default(),
														));
													}
												} else {
													ui.label(
														egui::RichText::new(
															language.text("discovery-no-invite"),
														)
														.color(colors.muted)
														.small(),
													);
												}
											},
										);
									});
								});
							ui.add_space(8.0);
						}
					});
				ui.add_space(8.0);
				ui.horizontal(|ui| {
					if let Some(offset) = state.discovery.previous()
						&& ui.button(language.text("discovery-previous")).clicked()
					{
						action = Some(Action::Reload(state.discovery.category(), offset));
					}
					if let Some(offset) = state.discovery.next()
						&& ui.button(language.text("discovery-next")).clicked()
					{
						action = Some(Action::Reload(state.discovery.category(), offset));
					}
					{
						let (matched, total) = state.discovery.matched();
						let (matched, total) = (matched.to_string(), total.to_string());
						ui.label(
							egui::RichText::new(crate::i18n::translate_args(
								"discovery-total",
								&[("count", &matched), ("total", &total)],
							))
							.color(colors.muted)
							.small(),
						);
					}
				});
			});
		match action {
			Some(Action::Reload(category, offset)) => {
				if let Some(command) = state.request_discovery(category, offset) {
					commands.push(command);
				}
			}
			Some(Action::Preview(code)) if !code.is_empty() => {
				// Only the service-supplied vanity code may enter the invite flow.
				self.join_server.open_with(state.generation, code);
			}
			None => {}
		}
	}
}

fn chip(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
	let colors = design::palette(ui);
	let button = egui::Button::new(egui::RichText::new(label).size(13.0))
		.corner_radius(14)
		.min_size(egui::vec2(0.0, 28.0));
	let button = if selected {
		button
			.fill(colors.accent)
			.stroke(egui::Stroke::new(1.0, colors.accent))
	} else {
		button
			.fill(colors.raised)
			.stroke(egui::Stroke::new(1.0, colors.border))
	};
	ui.add(button).clicked()
}
