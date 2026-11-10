//! The Discord Quests page: the account's current quests, their progress, and the two
//! actions — accept and claim.
//!
//! Everything here draws state the client core owns. The page only asks for a read and sends
//! an action; no quest data is cached in the view. The wire shape is unofficial
//! (userdoccers), so an unknown task type falls back to a generic local label instead of
//! showing remote text.

use crate::{MessagingUi, design, i18n, local_time};
use client_core::{Command, State, quests::Status};
use egui::RichText;
use model::Id;
use std::cell::Cell;

/// An action the user asked for; it is applied after the page is drawn.
#[derive(Clone, Copy)]
enum Pending {
	Accept(Id),
	Claim(Id),
}

/// One quest, copied out of the core before the page is drawn.
struct Row {
	id: Id,
	name: String,
	game: String,
	publisher: String,
	expires: Option<i64>,
	reward: Option<String>,
	primary: Option<u32>,
	status: model::quests::Status,
	tasks: Vec<(String, u64, u64, bool)>,
	pending: bool,
}

impl MessagingUi {
	/// The account's quests, with the accept and claim actions.
	pub(super) fn quests_page(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let language = self.language;
		let pending = Cell::new(None);
		let mut reload = false;
		egui::Frame::new()
			.inner_margin(egui::Margin::symmetric(24, 16))
			.show(ui, |ui| {
				ui.horizontal(|ui| {
					ui.label(
						design::semibold(ui, language.text("quests"), 20.0)
							.color(colors.text_strong),
					);
					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						if ui.button(language.text("quests-refresh")).clicked() {
							reload = true;
						}
					});
				});
				ui.add_space(4.0);
				// The list is read once per visit; a list that was never read is asked for here.
				let status = state.quests_status();
				if status == Status::Idle {
					reload = true;
				}
				if let Some(error) = state.quests_error() {
					ui.label(RichText::new(i18n::translate_if_key(error)).color(colors.warning));
				}
				if let Some(until) = state.quests_blocked_until()
					&& let Some(when) = local_time::discord_timestamp(until, b'f')
				{
					ui.label(
						RichText::new(crate::i18n::translate_args(
							"quests-blocked",
							&[("when", when.as_str())],
						))
						.color(colors.warning)
						.small(),
					);
				}
				if (reload || status == Status::Loading) && state.quests().is_empty() {
					ui.add_space(24.0);
					ui.label(RichText::new(language.text("quests-loading")).color(colors.muted));
					return;
				}
				// The list is read before it is drawn, so the rows are copied out first.
				let rows: Vec<Row> = state
					.quests()
					.iter()
					.map(|quest| Row {
						id: quest.id,
						name: quest.name.clone(),
						game: quest.game.clone(),
						publisher: quest.publisher.clone(),
						expires: quest.expires_at,
						reward: quest.reward.clone(),
						primary: quest.primary,
						status: quest.status(),
						tasks: quest
							.tasks
							.iter()
							.map(|task| {
								(task.kind.clone(), task.value, task.target, task.completed)
							})
							.collect(),
						pending: state.quests.action_pending(quest.id),
					})
					.collect();
				if rows.is_empty() {
					ui.add_space(24.0);
					ui.label(RichText::new(language.text("quests-empty")).color(colors.muted));
					return;
				}
				ui.add_space(8.0);
				egui::ScrollArea::vertical()
					.id_salt("quests-page")
					.auto_shrink([false, false])
					.show(ui, |ui| {
						for row in rows {
							let accent = row
								.primary
								.map(|value| {
									egui::Color32::from_rgb(
										(value >> 16) as u8,
										(value >> 8) as u8,
										value as u8,
									)
								})
								.unwrap_or(colors.accent);
							egui::Frame::new()
								.fill(colors.raised)
								.corner_radius(8)
								.inner_margin(egui::Margin::symmetric(12, 10))
								.show(ui, |ui| {
									ui.horizontal(|ui| {
										// A short gradient-like accent, standing in for the hero art.
										let (rect, _) = ui.allocate_exact_size(
											egui::vec2(4.0, 40.0),
											egui::Sense::hover(),
										);
										ui.painter().rect_filled(rect, 2, accent);
										ui.vertical(|ui| {
											ui.label(
												RichText::new(row.name.clone())
													.color(colors.text_strong)
													.strong(),
											);
											if !row.game.is_empty() {
												ui.label(
													RichText::new(row.game.clone())
														.color(colors.text),
												);
											}
											if !row.publisher.is_empty() {
												ui.label(
													RichText::new(row.publisher.clone())
														.color(colors.muted)
														.small(),
												);
											}
										});
									});
									ui.add_space(4.0);
									for (kind, value, target, completed) in &row.tasks {
										let label = language.text(task_key(kind));
										ui.label(RichText::new(label).color(colors.text).small());
										let fraction =
											if *completed || (*target > 0 && value >= target) {
												1.0
											} else if *target == 0 {
												0.0
											} else {
												(*value as f32 / *target as f32).clamp(0.0, 1.0)
											};
										ui.add(
											egui::ProgressBar::new(fraction)
												.desired_height(6.0)
												.fill(accent),
										);
										ui.label(
											RichText::new(format!(
												"{} / {}",
												(*value).min(*target),
												target
											))
											.color(colors.muted)
											.small(),
										);
									}
									if let Some(reward) = row.reward.clone() {
										ui.label(
											RichText::new(crate::i18n::translate_args(
												"quests-reward",
												&[("reward", reward.as_str())],
											))
											.color(colors.text)
											.small(),
										);
									}
									if let Some(expires) = row.expires
										&& let Some(when) =
											local_time::discord_timestamp(expires, b'f')
									{
										ui.label(
											RichText::new(crate::i18n::translate_args(
												"quests-expires",
												&[("when", when.as_str())],
											))
											.color(colors.muted)
											.small(),
										);
									}
									match row.status {
										model::quests::Status::Available => {
											if row.pending {
												ui.label(
													RichText::new(language.text("quests-pending"))
														.color(colors.muted),
												);
											} else if ui
												.add_sized(
													[ui.available_width(), 30.0],
													egui::Button::new(
														RichText::new(
															language.text("quests-accept"),
														)
														.color(colors.accent_text),
													)
													.fill(accent)
													.corner_radius(6),
												)
												.clicked()
											{
												pending.set(Some(Pending::Accept(row.id)));
											}
										}
										model::quests::Status::Enrolled => {
											ui.label(
												RichText::new(language.text("quests-in-progress"))
													.color(colors.muted),
											);
										}
										model::quests::Status::Completed => {
											if row.pending {
												ui.label(
													RichText::new(language.text("quests-pending"))
														.color(colors.muted),
												);
											} else if ui
												.add_sized(
													[ui.available_width(), 30.0],
													egui::Button::new(
														RichText::new(
															language.text("quests-claim"),
														)
														.color(colors.accent_text),
													)
													.fill(accent)
													.corner_radius(6),
												)
												.clicked()
											{
												pending.set(Some(Pending::Claim(row.id)));
											}
										}
										model::quests::Status::Claimed => {
											ui.label(
												RichText::new(language.text("quests-claimed"))
													.color(colors.muted),
											);
										}
									}
								});
							ui.add_space(8.0);
						}
					});
			});
		if reload && let Some(command) = state.request_quests() {
			commands.push(command);
		}
		match pending.get() {
			Some(Pending::Accept(quest)) => {
				if let Some(command) = state.request_enroll_quest(quest) {
					commands.push(command);
				}
			}
			Some(Pending::Claim(quest)) => {
				if let Some(command) = state.request_claim_quest(quest) {
					commands.push(command);
				}
			}
			None => {}
		}
	}
}

/// The local label for one service task type.
fn task_key(kind: &str) -> &'static str {
	match kind {
		"PLAY_ON_DESKTOP" | "PLAY_ON_DESKTOP_V2" => "quests-task-play-on-desktop",
		"STREAM_ON_DESKTOP" => "quests-task-stream-on-desktop",
		"PLAY_ON_XBOX" => "quests-task-play-on-xbox",
		"PLAY_ON_PLAYSTATION" => "quests-task-play-on-playstation",
		"WATCH_VIDEO" => "quests-task-watch-video",
		"WATCH_VIDEO_ON_MOBILE" => "quests-task-watch-video-on-mobile",
		"PLAY_ACTIVITY" => "quests-task-play-activity",
		"ACHIEVEMENT_IN_GAME" => "quests-task-achievement-in-game",
		"ACHIEVEMENT_IN_ACTIVITY" => "quests-task-achievement-in-activity",
		_ => "quests-task-generic",
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unknown_task_types_fall_back_to_a_local_label() {
		assert_eq!(task_key("PLAY_ON_DESKTOP"), "quests-task-play-on-desktop");
		assert_eq!(task_key("SOMETHING_NEW"), "quests-task-generic");
	}
}
