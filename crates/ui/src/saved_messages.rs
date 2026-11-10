//! Saved messages and reminders: a page in the home surface plus the message-menu actions.
use crate::{MessagingUi, design, i18n, local_time};
use client_core::{Command, State};
use egui::RichText;
use model::Id;

#[derive(Clone, Copy)]
enum Intent {
	Open(String),
	Remove(Id, Id),
}

impl MessagingUi {
	pub(super) fn saved_messages_page(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let language = self.language;
		let mut intent = None;
		let mut reload = false;
		// The list is read once per account; a failed read waits for an explicit retry.
		if self.saved_requested_generation != Some(state.generation) {
			self.saved_requested_generation = Some(state.generation);
			self.saved_requested = false;
		}
		if state.saved_messages.is_empty()
			&& !state.saved_messages.loading()
			&& state.saved_messages.list_error().is_none()
			&& !self.saved_requested
		{
			self.saved_requested = true;
			reload = true;
		}
		egui::Frame::new()
			.inner_margin(egui::Margin::symmetric(24, 16))
			.show(ui, |ui| {
				ui.horizontal(|ui| {
					ui.label(
						design::semibold(ui, language.text("saved-messages"), 20.0)
							.color(colors.text_strong),
					);
					if !state.saved_messages.is_empty() {
						let count = state.saved_messages.len();
						ui.label(
							RichText::new(i18n::translate_count(
								"saved-messages-count",
								count,
								&[],
							))
							.color(colors.muted)
							.small(),
						);
					}
					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						if ui.button(language.text("saved-messages-refresh")).clicked() {
							reload = true;
						}
					});
				});
				ui.label(
					RichText::new(language.text("saved-messages-description"))
						.color(colors.muted)
						.small(),
				);
				if let Some(error) = state.saved_messages.error() {
					ui.label(RichText::new(i18n::translate_if_key(error)).color(colors.danger));
				}
				if let Some(error) = state.saved_messages.error() {
					ui.label(RichText::new(i18n::translate_if_key(error)).color(colors.danger));
				}
				if let Some(error) = state.saved_messages.list_error() {
					ui.label(RichText::new(i18n::translate_if_key(error)).color(colors.warning));
					if ui.button(language.text("saved-messages-retry")).clicked() {
						reload = true;
					}
				}
				ui.add_space(12.0);
				ui.add(
					egui::TextEdit::singleline(&mut state.saved_messages.query)
						.hint_text(language.text("saved-messages-search"))
						.desired_width(f32::INFINITY),
				);
				ui.add_space(12.0);
				if state.saved_messages.loading() && state.saved_messages.is_empty() {
					ui.label(
						RichText::new(language.text("saved-messages-loading")).color(colors.muted),
					);
					return;
				}
				if state.saved_messages.is_empty() {
					ui.add_space(24.0);
					ui.vertical_centered(|ui| {
						ui.label(
							design::semibold(ui, language.text("saved-messages-empty"), 15.0)
								.color(colors.text_strong),
						);
						ui.label(
							RichText::new(language.text("saved-messages-empty-hint"))
								.color(colors.muted)
								.small(),
						);
					});
					return;
				}
				let query = state.saved_messages.query.trim().to_lowercase();
				let rows: Vec<_> = state
					.saved_messages
					.rows()
					.filter(|row| {
						query.is_empty()
							|| row.preview.as_ref().is_some_and(|preview| {
								preview.content.to_lowercase().contains(&query)
									|| preview.author.to_lowercase().contains(&query)
							}) || row.channel_summary.to_lowercase().contains(&query)
					})
					.map(|row| {
						(
							row.clone(),
							state.saved_messages.pending(row.channel, row.message),
						)
					})
					.collect();
				egui::ScrollArea::vertical()
					.id_salt("saved-messages-page")
					.auto_shrink([false, false])
					.show(ui, |ui| {
						for (row, pending) in rows {
							egui::Frame::new()
								.fill(colors.raised)
								.corner_radius(8)
								.inner_margin(egui::Margin::symmetric(12, 10))
								.show(ui, |ui| {
									ui.horizontal(|ui| {
										ui.label(
											design::medium(ui, row.author_summary.clone(), 14.0)
												.color(colors.text_strong),
										);
										ui.with_layout(
											egui::Layout::right_to_left(egui::Align::Center),
											|ui| {
												if pending {
													ui.label(
														RichText::new(
															language.text("saved-messages-pending"),
														)
														.color(colors.muted)
														.small(),
													);
												} else if ui
													.small_button(
														language.text("saved-messages-remove"),
													)
													.clicked()
												{
													intent = Some(Intent::Remove(
														row.channel,
														row.message,
													));
												}
												if let Some(due) = row.due_at
													&& let Some(when) =
														local_time::discord_timestamp(due, b'f')
												{
													ui.label(
														RichText::new(language.text_with_args(
															"saved-messages-reminder",
															&[("when", when.as_str())],
														))
														.color(colors.accent)
														.small(),
													);
												}
											},
										);
									});
									if let Some(preview) = &row.preview {
										// Service text is untrusted: control characters are stripped
										// before it reaches layout, and spoilers stay masked.
										let text: String = preview
											.content
											.chars()
											.map(|c| if c.is_control() { ' ' } else { c })
											.take(512)
											.collect();
										ui.add(
											egui::Label::new(
												RichText::new(text).color(colors.text),
											)
											.truncate(),
										);
									} else {
										ui.label(
											RichText::new(
												language.text("saved-messages-unavailable"),
											)
											.color(colors.muted)
											.small(),
										);
									}
									ui.horizontal(|ui| {
										ui.label(
											RichText::new(row.channel_summary.clone())
												.color(colors.muted)
												.small(),
										);
										ui.with_layout(
											egui::Layout::right_to_left(egui::Align::Center),
											|ui| {
												if ui
													.small_button(
														language.text("saved-messages-open"),
													)
													.clicked()
												{
													intent = Some(Intent::Open(message_link(
														state,
														row.channel,
														row.message,
													)));
												}
											},
										);
									});
								});
							ui.add_space(6.0);
						}
					});
			});
		match intent {
			Some(Intent::Open(link)) => self.timeline.opening = Some(link),
			Some(Intent::Remove(channel, message)) => {
				if let Some(command) = state.remove_saved_message(channel, message) {
					commands.push(command);
				}
			}
			None => {}
		}
		if reload && let Some(command) = state.load_saved_messages() {
			commands.push(command);
		}
	}
}

/// The same link form the timeline follows for a channel reference.
fn message_link(state: &State, channel: Id, message: Id) -> String {
	let guild = state
		.channel(channel)
		.and_then(|channel| channel.guild)
		.map_or("@me".to_owned(), |guild| guild.to_string());
	format!("https://discord.com/channels/{guild}/{channel}/{message}")
}
