//! Soundboard picker and voice channel status editor. Session-only; nothing is persisted.
use crate::{design, i18n};
use client_core::{
	Command, State,
	soundboard::{MAX_STATUS_CHARS, PreviewState, VoiceStatuses},
};
use model::Id;

#[derive(Default)]
pub(super) struct SoundboardUi {
	open: bool,
	/// A new account starts with a closed popup.
	generation: Option<u64>,
	channel: Option<Id>,
	/// The click that opened the popup lands this frame; a later click outside closes it.
	opened_frame: bool,
	status_editing: bool,
	status_draft: String,
	/// Kept so a stale answer cannot close a newer editor.
	status_request: u64,
	status_pending: bool,
	status_error: Option<&'static str>,
}
impl SoundboardUi {
	pub(super) fn close(&mut self) {
		self.open = false;
		self.status_editing = false;
	}
	pub(super) fn is_open(&self, channel: Id) -> bool {
		self.open && self.channel == Some(channel)
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
		if self.generation != Some(state.generation) {
			*self = Self {
				generation: Some(state.generation),
				..Default::default()
			};
		}
		if self.channel.is_some_and(|previous| previous != channel) {
			self.close();
			state.soundboard.stop_preview();
			self.status_pending = false;
			self.status_error = None;
		}
		if let Some((request, error)) = state.voice_statuses.take_settled()
			&& request == self.status_request
		{
			self.status_pending = false;
			self.status_error = error;
			if error.is_none() {
				self.status_editing = false;
			}
		}
		if response.clicked() {
			if self.is_open(channel) {
				self.close();
				state.soundboard.stop_preview();
			} else {
				self.open = true;
				self.opened_frame = true;
				self.channel = Some(channel);
				let guild = state.channel(channel).and_then(|c| c.guild);
				// A catalog belongs to one guild; switching guilds reloads even when it is full.
				if state.soundboard.guild != guild
					|| (state.soundboard.sounds.is_empty()
						&& !state.soundboard.busy
						&& state.soundboard.error.is_none())
				{
					self.reload(state, guild, commands);
				}
			}
		}
		if !self.is_open(channel) {
			return;
		}
		let anchor = response.rect;
		let area = egui::Area::new(response.id.with("soundboard-popup"))
			.order(egui::Order::Foreground)
			.fixed_pos(egui::pos2(
				anchor.right() - POPUP_WIDTH,
				(anchor.top() - 8.0 - POPUP_HEIGHT).max(8.0),
			))
			.constrain(true)
			.show(ctx, |ui| {
				egui::Frame::popup(ui.style()).show(ui, |ui| {
					ui.set_width(POPUP_WIDTH);
					self.contents(ui, state, channel, commands);
				});
			});
		let outside = !std::mem::take(&mut self.opened_frame)
			&& ctx.input(|i| {
				i.pointer.any_pressed()
					&& i.pointer.interact_pos().is_some_and(|pos| {
						!area.response.rect.contains(pos) && !anchor.contains(pos)
					})
			});
		if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
			self.close();
		}
		if !self.open {
			state.soundboard.stop_preview();
		}
	}

	fn reload(&mut self, state: &mut State, guild: Option<Id>, commands: &mut Vec<Command>) {
		let request = state.soundboard.request.wrapping_add(1);
		// Marked busy here so a second frame cannot queue the same load again.
		state.soundboard.begin(guild, request);
		commands.push(Command::Soundboard { guild, request });
	}

	fn contents(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		channel: Id,
		commands: &mut Vec<Command>,
	) {
		let colors = design::palette(ui);
		let mut reload = false;
		ui.horizontal(|ui| {
			ui.label(
				design::semibold(ui, i18n::translate("soundboard-ui-title"), 14.0)
					.color(colors.text_strong),
			);
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				if ui
					.small_button(i18n::translate("soundboard-ui-close"))
					.clicked()
				{
					self.close();
				}
				reload = ui
					.add_enabled(
						!state.soundboard.busy,
						egui::Button::new(i18n::translate("soundboard-ui-refresh")).small(),
					)
					.clicked();
			});
		});
		ui.add_space(6.0);
		ui.add(
			egui::TextEdit::singleline(&mut state.soundboard.query)
				.hint_text(i18n::translate("soundboard-ui-search"))
				.char_limit(100)
				.desired_width(f32::INFINITY),
		);
		ui.add_space(6.0);
		if let Some(error) = state.soundboard.error {
			ui.label(egui::RichText::new(i18n::translate_if_key(error)).color(colors.danger));
		}
		if let Some(preview) = &state.soundboard.preview
			&& let PreviewState::Failed(error) = preview.state
		{
			ui.label(egui::RichText::new(i18n::translate_if_key(error)).color(colors.danger));
		}
		let mut send = None;
		let mut preview = None;
		let mut stop = false;
		egui::ScrollArea::vertical()
			.id_salt(("soundboard-list", channel))
			.max_height(210.0)
			.auto_shrink([false, true])
			.show(ui, |ui| {
				if state.soundboard.busy {
					ui.horizontal(|ui| {
						ui.spinner();
						ui.label(i18n::translate("soundboard-ui-loading"));
					});
					return;
				}
				let rows = state.soundboard.matches();
				if rows.is_empty() {
					let key = if state.soundboard.sounds.is_empty() {
						"soundboard-ui-empty"
					} else {
						"soundboard-ui-no-results"
					};
					ui.label(egui::RichText::new(i18n::translate(key)).color(colors.muted));
					return;
				}
				for sound in rows {
					let playing = state.soundboard.preview.as_ref().is_some_and(|row| {
						row.sound == sound.id
							&& matches!(row.state, PreviewState::Loading | PreviewState::Playing)
					});
					ui.horizontal(|ui| {
						let key = if playing {
							"soundboard-ui-stop"
						} else {
							"soundboard-ui-preview"
						};
						if ui
							.add(egui::Button::new(i18n::translate(key)).small())
							.clicked()
						{
							if playing {
								stop = true;
							} else {
								preview = Some(sound.clone());
							}
						}
						let enabled = state.can_send_soundboard(channel, sound);
						let button = ui.add_enabled(
							enabled,
							egui::Button::new(&sound.name)
								.min_size(egui::vec2(ui.available_width(), 28.0)),
						);
						let key = if enabled {
							"soundboard-ui-send"
						} else {
							"soundboard-ui-send-unavailable"
						};
						if button.on_hover_text(i18n::translate(key)).clicked() {
							send = Some(sound.id);
						}
					});
					ui.add_space(2.0);
				}
			});
		if stop {
			state.soundboard.stop_preview();
		}
		if let Some(sound) = preview {
			state.soundboard.begin_preview(&sound);
		}
		if let Some(sound) = send
			&& let Some(command) = state.send_soundboard(channel, sound)
		{
			commands.push(command);
		}
		if reload {
			let guild = state.channel(channel).and_then(|c| c.guild);
			self.reload(state, guild, commands);
		}
		ui.separator();
		self.status_section(ui, state, channel, commands);
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
		if let Some(error) = self.status_error {
			ui.label(egui::RichText::new(i18n::translate_if_key(error)).color(colors.danger));
		}
		ui.add_enabled_ui(!self.status_pending, |ui| {
			if !self.status_editing {
				ui.label(
					egui::RichText::new(
						current
							.clone()
							.unwrap_or_else(|| i18n::translate("soundboard-ui-status-placeholder")),
					)
					.color(colors.muted),
				);
				ui.horizontal(|ui| {
					if ui
						.small_button(i18n::translate("soundboard-ui-status-edit"))
						.clicked()
					{
						self.status_editing = true;
						self.status_draft = current.clone().unwrap_or_default();
						self.status_error = None;
					}
					if current.is_some()
						&& ui
							.small_button(i18n::translate("soundboard-ui-status-clear"))
							.clicked()
					{
						self.save_status(channel, None, commands);
					}
				});
				return;
			}
			ui.add(
				egui::TextEdit::singleline(&mut self.status_draft)
					.hint_text(i18n::translate("soundboard-ui-status-hint"))
					.char_limit(MAX_STATUS_CHARS)
					.desired_width(f32::INFINITY),
			);
			ui.horizontal(|ui| {
				let valid = VoiceStatuses::valid_status(&self.status_draft);
				if ui
					.add_enabled(
						valid,
						egui::Button::new(i18n::translate("soundboard-ui-status-save")),
					)
					.clicked()
				{
					let status = self.status_draft.trim().to_owned();
					self.save_status(channel, (!status.is_empty()).then_some(status), commands);
				}
			});
			if ui
				.small_button(i18n::translate("soundboard-ui-status-cancel"))
				.clicked()
			{
				self.status_editing = false;
			}
		});
		if self.status_pending {
			ui.spinner();
		}
	}

	fn save_status(&mut self, channel: Id, status: Option<String>, commands: &mut Vec<Command>) {
		self.status_request = self.status_request.wrapping_add(1);
		self.status_pending = true;
		self.status_error = None;
		commands.push(Command::SetVoiceStatus {
			channel,
			status,
			request: self.status_request,
		});
	}

	/// The status text shown above the call controls, if this channel has one.
	pub(super) fn channel_line(state: &State, channel: Id) -> Option<&str> {
		state.voice_statuses.get(channel)
	}
}

const POPUP_WIDTH: f32 = 320.0;
const POPUP_HEIGHT: f32 = 380.0;
