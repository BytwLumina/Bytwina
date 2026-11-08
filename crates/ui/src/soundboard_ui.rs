//! Soundboard picker and voice channel status editor. Session-only; nothing is persisted.
use crate::design;
use client_core::{Command, State};
use model::Id;

#[derive(Default)]
pub(super) struct SoundboardUi {
	open: bool,
	/// The channel the popup belongs to; switching channels closes it.
	channel: Option<Id>,
	status_editing: bool,
	status_draft: String,
	/// Kept so a stale answer cannot close a newer editor.
	status_request: u64,
}
impl SoundboardUi {
	pub(super) fn close(&mut self) {
		self.open = false;
		self.status_editing = false;
	}
	pub(super) fn is_open(&self, channel: Id) -> bool {
		self.open && self.channel == Some(channel)
	}
	/// Called when the status write for `request` finished.
	pub(super) fn status_finished(&mut self, request: u64) {
		if request == self.status_request {
			self.status_editing = false;
		}
	}

	/// The launcher button beside the call controls, plus its popup.
	pub(super) fn launcher(
		&mut self,
		response: &egui::Response,
		ctx: &egui::Context,
		state: &mut State,
		channel: Id,
		commands: &mut Vec<Command>,
	) {
		// Close the editor as soon as the write settles, whatever its outcome.
		if let Some((request, _)) = state.voice_statuses.take_settled() {
			self.status_finished(request);
		}
		if response.clicked() {
			if self.is_open(channel) {
				self.close();
			} else {
				self.open = true;
				self.channel = Some(channel);
				if state.soundboard.sounds.is_empty()
					&& !state.soundboard.busy
					&& state.soundboard.error.is_none()
				{
					let guild = state.channel(channel).and_then(|c| c.guild);
					let request = state.soundboard.request.wrapping_add(1);
					// Marked busy here so a second frame cannot queue the same load again.
					state.soundboard.begin(request);
					commands.push(Command::Soundboard { guild, request });
				}
			}
		}
		if !self.is_open(channel) {
			return;
		}
		let anchor = response.rect;
		let mut sent = None;
		egui::Area::new(response.id.with("soundboard-popup"))
			.order(egui::Order::Foreground)
			.fixed_pos(egui::pos2(
				anchor.right() - POPUP_WIDTH,
				(anchor.top() - 8.0 - POPUP_HEIGHT).max(8.0),
			))
			.constrain(true)
			.show(ctx, |ui| {
				egui::Frame::popup(ui.style()).show(ui, |ui| {
					ui.set_width(POPUP_WIDTH);
					ui.set_max_height(POPUP_HEIGHT);
					sent = self.contents(ui, state, channel, commands);
				});
			});
		if let Some(sound) = sent {
			// Marked here, where the state is mutable, so the tile disables immediately. The
			// request id is the one the popup put on the command.
			let request = state.soundboard.send_request.wrapping_add(1);
			state.soundboard.begin_send(sound, request);
		}
	}

	fn contents(
		&mut self,
		ui: &mut egui::Ui,
		state: &State,
		channel: Id,
		commands: &mut Vec<Command>,
	) -> Option<Id> {
		let mut closed = false;
		let mut send = None;
		let colors = design::palette(ui);
		ui.horizontal(|ui| {
			ui.label(
				design::semibold(ui, crate::i18n::translate("soundboard-ui-title"), 14.0)
					.color(colors.text_strong),
			);
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				if ui
					.add(egui::Button::new(crate::i18n::translate("soundboard-ui-close")).small())
					.on_hover_text(crate::i18n::translate("soundboard-ui-close-popup"))
					.clicked()
				{
					closed = true;
				}
			});
		});
		ui.add_space(4.0);
		ui.separator();
		egui::ScrollArea::vertical()
			.id_salt(("soundboard-list", channel))
			.max_height(220.0)
			.auto_shrink([false, true])
			.show(ui, |ui| {
				if state.soundboard.busy {
					ui.horizontal(|ui| {
						ui.add(egui::Spinner::new().size(14.0));
						ui.label(
							egui::RichText::new(crate::i18n::translate("soundboard-ui-loading"))
								.color(colors.muted),
						);
					});
					return None;
				}
				if let Some(error) = state.soundboard.error {
					ui.label(egui::RichText::new(error).color(colors.danger));
					return None;
				}
				if state.soundboard.sounds.is_empty() {
					ui.label(
						egui::RichText::new(crate::i18n::translate("soundboard-ui-empty"))
							.color(colors.muted),
					);
					return None;
				}
				for sound in state.soundboard.matches() {
					let sending = state.soundboard.sending == Some(sound.id);
					let own = sound.guild.is_some();
					let row = egui::Button::new(if own {
						format!(
							"{}  ·  {}",
							sound.name,
							crate::i18n::translate("soundboard-ui-own-guild")
						)
					} else {
						sound.name.clone()
					})
					.min_size(egui::vec2(ui.available_width(), 28.0));
					if ui
						.add_enabled(!sending, row)
						.on_hover_text(format!(
							"{} · volume {}%",
							sound.guild.map_or_else(
								|| crate::i18n::translate("soundboard-ui-default-source"),
								|_| crate::i18n::translate("soundboard-ui-own-guild")
							),
							(sound.volume * 100.0).round()
						))
						.clicked()
					{
						send = Some(sound.id);
						commands.push(Command::SendSoundboardSound {
							channel,
							sound: sound.id,
							source_guild: sound.guild,
							request: state.soundboard.send_request.wrapping_add(1),
						});
					}
					ui.add_space(2.0);
				}
			});
		ui.separator();
		self.status_section(ui, state, channel, commands);
		if closed {
			self.close();
		}
		send
	}

	fn status_section(
		&mut self,
		ui: &mut egui::Ui,
		state: &State,
		channel: Id,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let current = state.voice_statuses.get(channel).map(str::to_owned);
		if !self.status_editing {
			ui.horizontal(|ui| {
				ui.add(
					egui::Label::new(
						egui::RichText::new(current.as_deref().unwrap_or_else(|| {
							crate::i18n::translate("soundboard-ui-status-placeholder")
						}))
						.color(colors.muted),
					)
					.truncate(),
				);
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					if ui
						.small_button(crate::i18n::translate("soundboard-ui-status-edit"))
						.clicked()
					{
						self.status_editing = true;
						self.status_draft = current.clone().unwrap_or_default();
					}
					if current.is_some()
						&& ui
							.small_button(crate::i18n::translate("soundboard-ui-status-clear"))
							.clicked()
					{
						self.status_request = self.status_request.wrapping_add(1);
						commands.push(Command::SetVoiceStatus {
							channel,
							status: None,
							request: self.status_request,
						});
					}
				});
			});
			return;
		}
		ui.label(
			egui::RichText::new(crate::i18n::translate("soundboard-ui-status-placeholder"))
				.size(12.0)
				.color(colors.muted),
		);
		ui.add(
			egui::TextEdit::singleline(&mut self.status_draft)
				.hint_text(&crate::i18n::translate("soundboard-ui-status-hint"))
				.desired_width(f32::INFINITY),
		);
		ui.add_space(4.0);
		ui.horizontal(|ui| {
			let over =
				self.status_draft.chars().count() > client_core::soundboard::MAX_STATUS_CHARS;
			if over {
				ui.label(
					egui::RichText::new(crate::i18n::translate("soundboard-ui-status-too-long"))
						.color(colors.danger),
				);
			}
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				let valid = !over
					&& client_core::soundboard::VoiceStatuses::valid_status(&self.status_draft);
				if ui
					.add_enabled(
						valid,
						egui::Button::new(crate::i18n::translate("soundboard-ui-status-save")),
					)
					.clicked()
				{
					self.status_request = self.status_request.wrapping_add(1);
					let status = self.status_draft.trim().to_owned();
					commands.push(Command::SetVoiceStatus {
						channel,
						status: (!status.is_empty()).then_some(status),
						request: self.status_request,
					});
				}
				if ui
					.small_button(crate::i18n::translate("soundboard-ui-status-cancel"))
					.clicked()
				{
					self.status_editing = false;
				}
			});
		});
	}

	/// The status text shown above the call controls, if this channel has one.
	pub(super) fn channel_line(state: &State, channel: Id) -> Option<&str> {
		state.voice_statuses.get(channel)
	}
}

const POPUP_WIDTH: f32 = 288.0;
const POPUP_HEIGHT: f32 = 340.0;
