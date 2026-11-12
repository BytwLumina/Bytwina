//! One bounded, session-only draft for the account's global profile.
use crate::{avatars::Avatars, design, icons};
use client_core::{Command, State};
use model::{Id, ProfileEdit, UserProfile};

#[derive(Clone, PartialEq, Eq)]
struct Draft {
	name: String,
	bio: String,
	pronouns: String,
	color: Option<u32>,
	/// `Some(Some(uri))` uploads a new picture, `Some(None)` removes the current one.
	avatar: Option<Option<String>>,
	/// Same encoding as `avatar`, for the profile banner.
	banner: Option<Option<String>>,
	/// Chosen collectible SKUs; `None` means "unchanged" and `Some(None)` clears.
	decoration: Option<Option<Id>>,
	nameplate: Option<Option<Id>>,
	effect: Option<Option<Id>>,
}
impl Draft {
	fn from_profile(profile: &UserProfile) -> Self {
		Self {
			name: profile.global_name.clone().unwrap_or_default(),
			bio: profile.bio.clone(),
			pronouns: profile.pronouns.clone(),
			color: profile.accent_color,
			avatar: None,
			banner: None,
			decoration: None,
			nameplate: None,
			effect: None,
		}
	}
	fn changes(&self, profile: &UserProfile) -> ProfileEdit {
		let name = (!self.name.is_empty()).then(|| self.name.clone());
		ProfileEdit {
			global_name: (name != profile.global_name).then_some(name),
			bio: (self.bio != profile.bio).then(|| self.bio.clone()),
			pronouns: (self.pronouns != profile.pronouns).then(|| self.pronouns.clone()),
			accent_color: (self.color != profile.accent_color).then_some(self.color),
			avatar: match &self.avatar {
				// Removing a picture the account does not have is not a change.
				Some(None) if profile.user.avatar.is_none() => None,
				other => other.clone(),
			},
			banner: match &self.banner {
				Some(None) if profile.banner.is_none() => None,
				other => other.clone(),
			},
			avatar_decoration_sku: self.decoration.filter(|selected| {
				*selected
					!= profile
						.user
						.cosmetics
						.as_deref()
						.and_then(|cosmetics| cosmetics.decoration.as_ref())
						.map(|decoration| decoration.sku)
			}),
			nameplate_sku: self.nameplate.filter(|selected| {
				*selected
					!= profile
						.user
						.cosmetics
						.as_deref()
						.and_then(|cosmetics| cosmetics.nameplate.as_ref())
						.map(|nameplate| nameplate.sku)
			}),
			profile_effect_sku: self
				.effect
				.filter(|selected| *selected != profile.effect_sku),
		}
	}
	fn rebase(&mut self, before: &Self, fresh: &Self) {
		if self.name == before.name {
			self.name = fresh.name.clone();
		}
		if self.bio == before.bio {
			self.bio = fresh.bio.clone();
		}
		if self.pronouns == before.pronouns {
			self.pronouns = fresh.pronouns.clone();
		}
		if self.color == before.color {
			self.color = fresh.color;
		}
	}
}

/// What the preview card paints inside the avatar circle.
#[derive(Clone, Copy)]
enum Picture<'a> {
	/// The account's current picture, fetched like any other avatar.
	Remote,
	/// A locally chosen image that has not been saved yet.
	Pending(&'a egui::TextureHandle),
	/// A locally chosen image that was just saved; shown until the CDN copy is fetched.
	Saved(&'a egui::TextureHandle),
	/// Removal is pending, so the initial placeholder is shown.
	Removed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AvatarAction {
	Pick,
	Remove,
	Undo,
	BannerPick,
	BannerRemove,
	BannerUndo,
}

#[derive(Default)]
pub(super) struct Editor {
	generation: Option<u64>,
	draft: Option<Draft>,
	baseline: Option<Draft>,
	submitted: Option<u64>,
	saved: bool,
	preview_link: Option<String>,
	/// Set when the picture button is pressed; the desktop shell opens the native picker.
	pub avatar_request: Option<(u64, Id, u64)>,
	revision: u64,
	choosing: bool,
	/// True while the native picker is filling the banner instead of the avatar.
	choosing_banner: bool,
	pending_picture: Option<egui::TextureHandle>,
	/// Local copy of the last saved picture, keyed by the avatar hash the service returned.
	saved_picture: Option<(Option<String>, egui::TextureHandle)>,
}
impl Editor {
	/// Result of the native picker. Errors are returned so the caller can toast them.
	pub fn accept_avatar(
		&mut self,
		ctx: &egui::Context,
		request: (u64, Id, u64),
		result: Result<Option<(String, egui::ColorImage)>, &'static str>,
	) -> Result<(), &'static str> {
		if !self.choosing
			|| request
				!= (
					self.generation.unwrap_or_default(),
					request.1,
					self.revision,
				) {
			return Ok(());
		}
		let banner = self.choosing_banner;
		self.choosing = false;
		self.choosing_banner = false;
		match result {
			Ok(Some((uri, image))) => {
				if !model::valid_avatar_uri(&uri) || image.size[0] > 512 || image.size[1] > 512 {
					return Err("That picture is too large; choose a simpler image");
				}
				let Some(draft) = self.draft.as_mut() else {
					return Ok(());
				};
				if banner {
					draft.banner = Some(Some(uri));
					self.pending_picture = Some(ctx.load_texture(
						"profile-banner-preview",
						image,
						egui::TextureOptions::LINEAR,
					));
					self.saved = false;
					return Ok(());
				}
				draft.avatar = Some(Some(uri));
				self.pending_picture = Some(ctx.load_texture(
					"profile-avatar-preview",
					image,
					egui::TextureOptions::LINEAR,
				));
				self.saved = false;
				Ok(())
			}
			Ok(None) => Ok(()),
			Err(error) => Err(error),
		}
	}

	pub fn show(
		&mut self,
		ui: &mut egui::Ui,
		state: &mut State,
		avatars: &mut Avatars,
		commands: &mut Vec<Command>,
	) {
		if self.generation != Some(state.generation) {
			*self = Self {
				generation: Some(state.generation),
				..Self::default()
			};
		}
		if state.own_profile.data.is_none()
			&& !state.own_profile.loading
			&& state.own_profile.error.is_none()
			&& let Some(command) = state.load_own_profile()
		{
			commands.push(command);
		}
		// The decoration picker needs the owned list; load it once per session.
		if !state.demo
			&& !state.collectibles.loaded
			&& !state.collectibles.busy
			&& state.collectibles.error.is_none()
			&& state.auth == client_core::auth::AuthState::Authenticated
		{
			let request = state.collectibles.request.wrapping_add(1);
			state.collectibles.begin(request);
			commands.push(Command::OwnedCollectibles { request });
		}
		if !state.demo && state.collectibles.busy {
			ui.horizontal(|ui| {
				ui.add(egui::Spinner::new().size(14.0));
				ui.label(crate::i18n::translate("profile-edit-collectibles-loading"));
			});
		} else if let Some(error) = state.collectibles.error {
			design::notice(ui, design::Level::Error, error);
			if state.auth == client_core::auth::AuthState::Authenticated
				&& design::text_action(ui, &crate::i18n::translate("channel-menu-show-retry"))
					.clicked()
			{
				let request = state.collectibles.request.wrapping_add(1);
				state.collectibles.begin(request);
				commands.push(Command::OwnedCollectibles { request });
			}
		} else if !state.demo && state.collectibles.loaded && state.collectibles.owned.is_empty() {
			design::hint(
				ui,
				&crate::i18n::translate("profile-edit-collectibles-empty"),
			);
		}
		if self.submitted == Some(state.own_profile.request) && !state.own_profile.saving {
			self.submitted = None;
			if state.own_profile.error.is_none()
				&& let Some(profile) = &state.own_profile.data
			{
				let uploaded = self
					.draft
					.as_ref()
					.is_some_and(|draft| matches!(draft.avatar, Some(Some(_))));
				self.saved_picture = match (uploaded, self.pending_picture.take()) {
					(true, Some(texture)) => Some((profile.user.avatar.clone(), texture)),
					_ => None,
				};
				self.draft = Some(Draft::from_profile(profile));
				self.saved = true;
			}
		}
		let colors = design::palette(ui);
		if state.own_profile.loading {
			ui.horizontal(|ui| {
				ui.add(egui::Spinner::new().size(14.0));
				ui.label(
					egui::RichText::new(crate::i18n::translate(
						"profile-edit-show-loading-your-profile",
					))
					.size(13.0)
					.color(colors.muted),
				);
			});
		}
		if let Some(error) = state.own_profile.error {
			design::notice(ui, design::Level::Error, error);
			if !state.own_profile.loading
				&& !state.own_profile.saving
				&& design::text_action(
					ui,
					&crate::i18n::translate("profile-edit-show-reload-profile"),
				)
				.clicked() && let Some(command) = state.load_own_profile()
			{
				commands.push(command);
			}
		}
		let Some(profile) = state.own_profile.data.as_ref() else {
			return;
		};
		let fresh = Draft::from_profile(profile);
		if let (Some(draft), Some(before)) = (&mut self.draft, &self.baseline) {
			draft.rebase(before, &fresh);
		}
		self.baseline = Some(fresh);
		let draft = self
			.draft
			.get_or_insert_with(|| Draft::from_profile(profile));
		let before = draft.clone();
		let editable = !state.own_profile.loading && !state.own_profile.saving;
		let picture = match &draft.avatar {
			Some(Some(_)) => self
				.pending_picture
				.as_ref()
				.map_or(Picture::Remote, Picture::Pending),
			Some(None) => Picture::Removed,
			None => match &self.saved_picture {
				Some((hash, texture)) if hash.is_some() && *hash == profile.user.avatar => {
					Picture::Saved(texture)
				}
				_ => Picture::Remote,
			},
		};
		let mut action = None;
		let width = ui.available_width();
		if width >= 620.0 {
			ui.horizontal_top(|ui| {
				ui.spacing_mut().item_spacing.x = 24.0;
				ui.allocate_ui_with_layout(
					egui::vec2(width - 324.0, 0.0),
					egui::Layout::top_down(egui::Align::Min),
					|ui| {
						ui.add_enabled_ui(editable, |ui| {
							action =
								form(ui, draft, profile, self.choosing, &state.collectibles.owned);
						});
					},
				);
				ui.vertical(|ui| {
					ui.set_width(300.0);
					if let Some(picked) = preview(
						ui,
						draft,
						profile,
						avatars,
						(state.demo, &state.guilds),
						&mut self.preview_link,
						picture,
						editable && !self.choosing,
					) {
						action = Some(picked);
					}
				});
			});
		} else {
			ui.add_enabled_ui(editable, |ui| {
				action = form(ui, draft, profile, self.choosing, &state.collectibles.owned);
			});
		}
		match action {
			Some(AvatarAction::Pick) if editable && !self.choosing => {
				self.revision = self.revision.wrapping_add(1);
				self.choosing = true;
				self.avatar_request = Some((state.generation, profile.user.id, self.revision));
			}
			Some(AvatarAction::Remove) => {
				draft.avatar = Some(None);
				self.pending_picture = None;
			}
			Some(AvatarAction::Undo) => {
				draft.avatar = None;
				self.pending_picture = None;
			}
			Some(AvatarAction::BannerPick) if editable && !self.choosing => {
				self.revision = self.revision.wrapping_add(1);
				self.choosing = true;
				self.choosing_banner = true;
				self.avatar_request = Some((state.generation, profile.user.id, self.revision));
			}
			Some(AvatarAction::BannerRemove) => {
				draft.banner = Some(None);
			}
			Some(AvatarAction::BannerUndo) => {
				draft.banner = None;
			}
			_ => {}
		}
		if *draft != before {
			self.saved = false;
		}
		let changes = draft.changes(profile);
		let changed = changes != ProfileEdit::default();
		if !changes.valid() {
			design::notice(
				ui,
				design::Level::Error,
				&crate::i18n::translate(
					"profile-edit-show-check-character-limits-and-remove-control-characters-a-display-name",
				),
			);
		}
		ui.add_space(16.0);
		egui::Frame::new()
			.fill(colors.base)
			.corner_radius(8)
			.inner_margin(egui::Margin::symmetric(12, 8))
			.show(ui, |ui| {
				ui.set_width(ui.available_width());
				ui.horizontal(|ui| {
					ui.spacing_mut().item_spacing.x = 12.0;
					if state.own_profile.saving {
						ui.add(egui::Spinner::new().size(14.0));
						ui.label(
							egui::RichText::new(crate::i18n::translate(
								"profile-edit-show-saving-profile",
							))
							.size(13.0)
							.color(colors.muted),
						);
					} else if self.saved {
						ui.label(
							egui::RichText::new(crate::i18n::translate_if_key(if state.demo {
								"profile-edit-show-saved-in-preview"
							} else {
								"profile-edit-show-profile-saved"
							}))
							.size(13.0)
							.color(colors.positive),
						);
					} else if changed {
						ui.label(
							egui::RichText::new(crate::i18n::translate(
								"profile-edit-show-you-have-unsaved-changes",
							))
							.size(13.0)
							.color(colors.text),
						);
					}
					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						ui.add_enabled_ui(
							changed && changes.valid() && state.can_save_own_profile(),
							|ui| {
								if design::button(
									ui,
									&crate::i18n::translate("profile-edit-show-save-changes"),
									design::ButtonKind::Primary,
								)
								.clicked() && let Some(command) = state.save_own_profile(changes)
								{
									self.submitted = Some(state.own_profile.request);
									self.saved = false;
									commands.push(command);
								}
							},
						);
						ui.add_enabled_ui(changed && editable, |ui| {
							if design::button(
								ui,
								&crate::i18n::translate("profile-edit-show-cancel"),
								design::ButtonKind::Neutral,
							)
							.clicked()
							{
								self.draft =
									state.own_profile.data.as_ref().map(Draft::from_profile);
								self.pending_picture = None;
								self.saved = false;
							}
						});
					});
				});
			});
		if width < 620.0
			&& let (Some(draft), Some(profile)) = (&self.draft, &state.own_profile.data)
		{
			ui.add_space(16.0);
			let picture = match &draft.avatar {
				Some(Some(_)) => self
					.pending_picture
					.as_ref()
					.map_or(Picture::Remote, Picture::Pending),
				Some(None) => Picture::Removed,
				None => Picture::Remote,
			};
			preview(
				ui,
				draft,
				profile,
				avatars,
				(state.demo, &state.guilds),
				&mut self.preview_link,
				picture,
				false,
			);
		}
		crate::markdown::confirm_external_link(ui.ctx(), &mut self.preview_link, true);
		if !state.demo && !state.gateway_connected {
			design::hint(
				ui,
				&crate::i18n::translate("profile-edit-show-reconnect-to-save-your-profile"),
			);
		}
	}
}

fn form(
	ui: &mut egui::Ui,
	draft: &mut Draft,
	profile: &UserProfile,
	choosing: bool,
	owned: &[model::OwnedCollectible],
) -> Option<AvatarAction> {
	let colors = design::palette(ui);
	ui.spacing_mut().item_spacing.y = 6.0;
	let mut action = None;
	let has_picture = match &draft.avatar {
		Some(value) => value.is_some(),
		None => profile.user.avatar.is_some(),
	};
	design::row(
		ui,
		"profile-edit-form-profile-picture",
		Some(match &draft.avatar {
			Some(Some(_)) => "profile-edit-form-new-picture-chosen-save-to-upload-it",
			Some(None) => "profile-edit-form-your-picture-will-be-removed-when-you-save",
			None => "profile-edit-form-png-jpeg-gif-or-webp-up-to-8-mb-cropped",
		}),
		|ui| {
			ui.add_enabled_ui(!choosing, |ui| {
				if design::button(
					ui,
					&crate::i18n::translate("profile-edit-form-change"),
					design::ButtonKind::Outline,
				)
				.clicked()
				{
					action = Some(AvatarAction::Pick);
				}
			});
			if draft.avatar.is_some() {
				if design::text_action(ui, &crate::i18n::translate("profile-edit-form-undo"))
					.clicked()
				{
					action = Some(AvatarAction::Undo);
				}
			} else if has_picture
				&& design::text_action(ui, &crate::i18n::translate("profile-edit-form-remove"))
					.clicked()
			{
				action = Some(AvatarAction::Remove);
			}
			if choosing {
				ui.add(egui::Spinner::new().size(14.0));
			}
		},
	);
	ui.add_space(10.0);
	field(
		ui,
		"profile-edit-form-display-name",
		"profile-display-name",
		&mut draft.name,
		model::MAX_PROFILE_NAME_CHARS,
		false,
	);
	ui.label(
		egui::RichText::new(crate::i18n::translate(
			"profile-edit-form-leave-blank-to-use-your-username",
		))
		.size(12.0)
		.color(colors.muted),
	);
	ui.add_space(8.0);
	field(
		ui,
		"profile-edit-form-pronouns",
		"profile-pronouns",
		&mut draft.pronouns,
		model::MAX_PROFILE_PRONOUNS_CHARS,
		false,
	);
	ui.add_space(8.0);
	field(
		ui,
		"profile-edit-form-about-me",
		"profile-about-me",
		&mut draft.bio,
		model::MAX_PROFILE_BIO_CHARS,
		true,
	);
	ui.add_space(8.0);
	let mut enabled = draft.color.is_some();
	ui.add_space(8.0);
	let has_banner = match &draft.banner {
		Some(value) => value.is_some(),
		None => profile.banner.is_some(),
	};
	design::row(
		ui,
		"profile-edit-form-banner",
		Some(match &draft.banner {
			Some(Some(_)) => "profile-edit-form-new-banner-chosen-save-to-upload-it",
			Some(None) => "profile-edit-form-your-banner-will-be-removed-when-you-save",
			None => "profile-edit-form-premium-accounts-can-add-a-banner",
		}),
		|ui| {
			ui.add_enabled_ui(!choosing, |ui| {
				if design::button(
					ui,
					&crate::i18n::translate("profile-edit-form-change"),
					design::ButtonKind::Outline,
				)
				.clicked()
				{
					action = Some(AvatarAction::BannerPick);
				}
			});
			if draft.banner.is_some() {
				if design::text_action(ui, &crate::i18n::translate("profile-edit-form-undo"))
					.clicked()
				{
					action = Some(AvatarAction::BannerUndo);
				}
			} else if has_banner
				&& design::text_action(ui, &crate::i18n::translate("profile-edit-form-remove"))
					.clicked()
			{
				action = Some(AvatarAction::BannerRemove);
			}
		},
	);
	ui.add_space(8.0);
	// Separate calls: the three selections are distinct fields borrowed mutably in turn.
	decor_row(
		ui,
		"profile-edit-form-decoration",
		model::CollectibleKind::Decoration,
		&mut draft.decoration,
		profile.user.cosmetics.as_deref().and_then(|cosmetics| {
			cosmetics
				.decoration
				.as_ref()
				.map(|decoration| decoration.sku)
		}),
		owned,
	);
	ui.add_space(6.0);
	decor_row(
		ui,
		"profile-edit-form-nameplate",
		model::CollectibleKind::Nameplate,
		&mut draft.nameplate,
		profile
			.user
			.cosmetics
			.as_deref()
			.and_then(|cosmetics| cosmetics.nameplate.as_ref().map(|nameplate| nameplate.sku)),
		owned,
	);
	ui.add_space(6.0);
	decor_row(
		ui,
		"profile-edit-form-profile-effect",
		model::CollectibleKind::Effect,
		&mut draft.effect,
		profile.effect_sku,
		owned,
	);
	ui.add_space(6.0);
	design::row(
		ui,
		"profile-edit-form-profile-color",
		Some("profile-edit-form-tints-your-banner-when-you-have-not-set-a-banner"),
		|ui| {
			if let Some(color) = &mut draft.color {
				let mut rgb = [(*color >> 16) as u8, (*color >> 8) as u8, *color as u8];
				if design::color_edit(ui, &mut rgb)
					.on_hover_text(crate::i18n::translate(
						"profile-edit-form-choose-profile-color",
					))
					.changed()
				{
					*color =
						(u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2]);
				}
				if design::text_action(ui, &crate::i18n::translate("profile-edit-form-use-default"))
					.clicked()
				{
					enabled = false;
				}
			} else if design::text_action(
				ui,
				&crate::i18n::translate("profile-edit-form-custom-color"),
			)
			.clicked()
			{
				enabled = true;
			}
		},
	);
	if enabled != draft.color.is_some() {
		// Start from the profile's own colors rather than the app accent.
		draft.color = enabled.then(|| {
			profile
				.accent_color
				.or(profile.theme_colors.map(|[top, _]| top))
				.unwrap_or(design::DEFAULT_PRIMARY_RGB)
		});
	}
	action
}

/// One owned-collectible picker: "Default" plus every owned product of a category.
fn decor_row(
	ui: &mut egui::Ui,
	label: &str,
	kind: model::CollectibleKind,
	slot: &mut Option<Option<Id>>,
	current: Option<Id>,
	owned: &[model::OwnedCollectible],
) {
	let chosen = slot.unwrap_or(current);
	let name = chosen
		.and_then(|sku| owned.iter().find(|item| item.sku == sku))
		.map_or_else(
			|| {
				crate::i18n::translate(if chosen.is_some() {
					"profile-edit-collectibles-current"
				} else {
					"profile-edit-form-none"
				})
			},
			|item| item.name.clone(),
		);
	design::row(ui, label, None, |ui| {
		egui::ComboBox::from_id_salt(("profile-decor", label))
			.selected_text(name)
			.width(180.0)
			.show_ui(ui, |ui| {
				if ui
					.selectable_value(
						slot,
						None,
						crate::i18n::translate("profile-edit-collectibles-keep-current"),
					)
					.clicked()
				{
					return;
				}
				if ui
					.selectable_value(
						slot,
						Some(None),
						crate::i18n::translate("profile-edit-form-none"),
					)
					.clicked()
				{
					return;
				}
				for item in owned.iter().filter(|item| item.kind == kind) {
					if ui
						.selectable_value(slot, Some(Some(item.sku)), item.name.as_str())
						.clicked()
					{
						return;
					}
				}
			});
	});
}

fn field(
	ui: &mut egui::Ui,
	label: &str,
	id: &str,
	value: &mut String,
	limit: usize,
	multiline: bool,
) {
	let colors = design::palette(ui);
	let label = crate::i18n::translate_if_key(label);
	let label_id = ui
		.horizontal(|ui| {
			let label = ui.label(design::eyebrow(ui, &label, colors.muted));
			if multiline {
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					ui.label(
						egui::RichText::new(format!("{} / {limit}", value.chars().count()))
							.size(11.0)
							.color(colors.muted),
					);
				});
			}
			label.id
		})
		.inner;
	let edit = if multiline {
		egui::TextEdit::multiline(value).desired_rows(4)
	} else {
		egui::TextEdit::singleline(value).align(egui::Align2::LEFT_CENTER)
	};
	design::input(
		ui,
		edit.id(egui::Id::unique(id))
			.char_limit(limit)
			.font(egui::FontId::proportional(15.0))
			.text_color(colors.text),
	)
	.labelled_by(label_id);
}

#[allow(clippy::too_many_arguments)]
fn preview(
	ui: &mut egui::Ui,
	draft: &Draft,
	profile: &UserProfile,
	avatars: &mut Avatars,
	media: (bool, &[model::Guild]),
	opening: &mut Option<String>,
	picture: Picture<'_>,
	clickable: bool,
) -> Option<AvatarAction> {
	let (demo, guilds) = media;
	let colors = design::palette(ui);
	let mut action = None;
	ui.label(design::eyebrow(
		ui,
		crate::i18n::translate("profile-edit-preview-preview"),
		colors.muted,
	));
	ui.add_space(4.0);
	// Paint with the profile's own colors, as other people see the card, not the app theme.
	let theme = crate::profiles::Theme::new(&colors, profile.theme_colors);
	let rgb = |c: u32| egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
	ui.scope(|ui| {
		let frame_back = ui.painter().add(egui::Shape::Noop);
		let background = ui.painter().add(egui::Shape::Noop);
		ui.visuals_mut().override_text_color = Some(theme.text);
		ui.visuals_mut().hyperlink_color = theme.link;
		ui.set_width(ui.available_width());
		ui.spacing_mut().item_spacing.y = 0.0;
		{
			let (banner, _) = ui
				.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
			let corner = egui::CornerRadius {
				nw: 12,
				ne: 12,
				sw: 0,
				se: 0,
			};
			if profile.banner.is_some() {
				// The banner image covers its color, so the saved profile paints it as is.
				avatars.paint_banner(ui, profile, banner, corner, demo);
			} else {
				let color = profile
					.theme_colors
					.map(crate::profiles::Theme::banner)
					.or_else(|| draft.color.map(rgb))
					.unwrap_or(colors.raised);
				ui.painter().rect_filled(banner, corner, color);
			}
			let avatar = egui::Rect::from_min_size(
				banner.left_bottom() + egui::vec2(16.0, -40.0),
				egui::Vec2::splat(80.0),
			);
			ui.painter()
				.circle_filled(avatar.center(), 46.0, theme.card);
			let current_decoration = profile
				.user
				.cosmetics
				.as_deref()
				.and_then(|cosmetics| cosmetics.decoration.as_ref())
				.map(|decoration| decoration.sku);
			let show_decoration = draft
				.decoration
				.is_none_or(|choice| choice == current_decoration);
			let response =
				ui.scope_builder(
					egui::UiBuilder::new().max_rect(avatar),
					|ui| match picture {
						Picture::Pending(texture) | Picture::Saved(texture) => ui.add(
							egui::Image::new(texture)
								.fit_to_exact_size(egui::Vec2::splat(80.0))
								.corner_radius(40.0),
						),
						Picture::Removed => design::avatar(ui, &profile.user.name, 80.0),
						Picture::Remote => avatars.with_avatar_animation(true, |avatars| {
							if show_decoration {
								avatars.show_plain(ui, &profile.user, 80.0, demo)
							} else {
								let mut user = profile.user.clone();
								if let Some(cosmetics) = &mut user.cosmetics {
									cosmetics.decoration = None;
								}
								avatars.show_plain(ui, &user, 80.0, demo)
							}
						}),
					},
				);
			if show_decoration && !matches!(picture, Picture::Remote) {
				avatars.paint_decoration(ui, &profile.user, 80.0, avatar, demo);
			}
			if clickable {
				let hit = ui.interact(
					avatar,
					ui.scope_id().with("change-avatar"),
					egui::Sense::click(),
				);
				let hit = hit.on_hover_text(crate::i18n::translate(
					"profile-edit-preview-change-profile-picture",
				));
				if hit.hovered() || hit.has_focus() {
					ui.painter().circle_filled(
						avatar.center(),
						40.0,
						egui::Color32::from_black_alpha(120),
					);
					icons::paint(
						ui.painter(),
						icons::Icon::Pencil,
						egui::Rect::from_center_size(avatar.center(), egui::Vec2::splat(22.0)),
						egui::Color32::WHITE,
					);
				}
				if hit.clicked() {
					action = Some(AvatarAction::Pick);
				}
			}
			drop(response);
			ui.add_space((avatar.bottom() + 12.0 - ui.cursor().top()).max(0.0));
			egui::Frame::new()
				.inner_margin(egui::Margin {
					left: 12,
					right: 12,
					top: 0,
					bottom: 12,
				})
				.show(ui, |ui| {
					egui::Frame::new()
						.fill(theme.panel)
						.corner_radius(8)
						.inner_margin(12)
						.show(ui, |ui| {
							ui.set_width(ui.available_width());
							ui.set_min_height(138.0);
							ui.spacing_mut().item_spacing.y = 4.0;
							let name = if draft.name.is_empty() {
								&profile.username
							} else {
								&draft.name
							};
							ui.add(
								egui::Label::new(
									design::semibold(ui, name, 20.0).color(theme.text),
								)
								.wrap(),
							);
							ui.label(
								egui::RichText::new(&profile.username)
									.size(13.0)
									.color(theme.text),
							);
							if !draft.pronouns.is_empty() {
								ui.label(
									egui::RichText::new(&draft.pronouns)
										.size(12.0)
										.color(theme.muted),
								);
							}
							if !draft.bio.is_empty() {
								ui.add_space(8.0);
								ui.separator();
								ui.add_space(8.0);
								ui.label(design::eyebrow(
									ui,
									crate::i18n::translate("profile-edit-preview-about-me"),
									theme.text,
								));
								let mut mentions = crate::profiles::ProfileSession::default();
								crate::markdown::Formatted::parse(&draft.bio).show_with_images(
									ui,
									opening,
									&[],
									None,
									&mut mentions,
									(avatars, demo, guilds),
									crate::design::MessageCardSurface::Opaque,
								);
							}
						});
				});
		}
		let rect = ui.min_rect();
		ui.painter().set(background, theme.background(rect));
		crate::profiles::paint_collectibles(
			ui,
			avatars,
			profile,
			rect,
			frame_back,
			(
				true,
				None,
				true,
				draft
					.effect
					.is_none_or(|choice| choice == profile.effect_sku),
			),
			demo,
		);
	});
	action
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn draft_only_sends_changed_fields_and_can_clear_values() {
		let user = model::User {
			flags: model::AccountFlags::default(),
			id: model::Id(1),
			name: "Synthetic".into(),
			avatar: None,
			webhook: false,
			kind: Default::default(),
			discriminator: 0,
			primary_guild: None,
			cosmetics: None,
		};
		let mut profile = crate::profiles::synthetic(&user, None);
		profile.global_name = Some("Before".into());
		profile.accent_color = Some(0x123456);
		let mut draft = Draft::from_profile(&profile);
		assert!(draft.changes(&profile) == ProfileEdit::default());
		draft.name.clear();
		draft.bio.clear();
		draft.color = None;
		let changes = draft.changes(&profile);
		assert_eq!(changes.global_name, Some(None));
		assert_eq!(changes.bio, Some(String::new()));
		assert_eq!(changes.accent_color, Some(None));
		assert_eq!(changes.pronouns, None);
		assert!(changes.valid());
		// A reload retains edited values but adopts remote changes to untouched fields.
		let before = Draft::from_profile(&profile);
		profile.pronouns = "she/her".into();
		draft.rebase(&before, &Draft::from_profile(&profile));
		assert_eq!(draft.pronouns, "she/her");
		assert!(draft.name.is_empty() && draft.bio.is_empty());
		assert_eq!(draft.changes(&profile).pronouns, None);
		profile.user.cosmetics = Some(Box::new(test_support::synthetic_cosmetics()));
		let mut draft = Draft::from_profile(&profile);
		draft.decoration = Some(Some(Id(9101)));
		draft.nameplate = Some(Some(Id(9102)));
		draft.effect = Some(profile.effect_sku);
		assert!(draft.changes(&profile) == ProfileEdit::default());
		draft.decoration = Some(None);
		draft.effect = Some(Some(Id(5)));
		let changes = draft.changes(&profile);
		assert_eq!(changes.avatar_decoration_sku, Some(None));
		assert_eq!(changes.profile_effect_sku, Some(Some(Id(5))));
		assert_eq!(changes.nameplate_sku, None);
	}

	#[test]
	fn empty_owned_list_is_not_requested_again_on_each_frame() {
		let ctx = egui::Context::default();
		let mut state = test_support::demo_state();
		state.demo = false;
		state.auth = client_core::auth::AuthState::Authenticated;
		state.gateway_connected = true;
		let mut editor = Editor::default();
		let mut avatars = Avatars::default();
		let mut frame = |state: &mut State| {
			let mut commands = Vec::new();
			ctx.run_ui(Default::default(), |ui| {
				editor.show(ui, state, &mut avatars, &mut commands);
			})
			.drop_without_applying_deltas();
			commands
		};
		let commands = frame(&mut state);
		assert!(
			commands
				.iter()
				.any(|command| matches!(command, Command::OwnedCollectibles { .. }))
		);
		let request = state.collectibles.request;
		state.collectibles.finish(request, Ok(Vec::new()));
		for _ in 0..3 {
			assert!(
				!frame(&mut state)
					.iter()
					.any(|command| matches!(command, Command::OwnedCollectibles { .. }))
			);
		}
	}
	#[test]
	fn editor_saves_once_preserves_failed_draft_and_cancels_in_both_themes() {
		fn frame(
			ctx: &egui::Context,
			editor: &mut Editor,
			state: &mut State,
			avatars: &mut Avatars,
			width: f32,
			events: Vec<egui::Event>,
		) -> (Vec<Command>, Vec<(String, egui::Rect)>) {
			let mut commands = vec![];
			let mut output = ctx.run_ui(
				egui::RawInput {
					screen_rect: Some(egui::Rect::from_min_size(
						egui::Pos2::ZERO,
						egui::vec2(width, 1100.0),
					)),
					events,
					..Default::default()
				},
				|ui| editor.show(ui, state, avatars, &mut commands),
			);
			output.textures_delta.clear();
			fn text(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
				match shape {
					egui::Shape::Text(text) => labels.push((
						text.galley.job.text.clone(),
						text.galley.rect.translate(text.pos.to_vec2()),
					)),
					egui::Shape::Vec(shapes) => {
						for shape in shapes {
							text(shape, labels);
						}
					}
					_ => {}
				}
			}
			let mut labels = vec![];
			for shape in output.shapes {
				text(&shape.shape, &mut labels);
			}
			(commands, labels)
		}
		for theme in [egui::ThemePreference::Dark, egui::ThemePreference::Light] {
			for width in [320.0, 720.0] {
				let ctx = egui::Context::default();
				ctx.set_theme(theme);
				let mut editor = Editor::default();
				let mut avatars = Avatars::default();
				let mut state = test_support::demo_state();
				let (commands, _) =
					frame(&ctx, &mut editor, &mut state, &mut avatars, width, vec![]);
				assert_eq!(commands.len(), 1);
				let Command::EditProfile {
					user,
					request,
					changes: None,
					..
				} = commands.into_iter().next().unwrap()
				else {
					panic!("load expected")
				};
				state.apply(client_core::Envelope {
					generation: state.generation,
					event: client_core::Event::ProfileEdited {
						user,
						request,
						result: Ok(Box::new(crate::profiles::synthetic(
							state.user.as_ref().unwrap(),
							None,
						))),
					},
				});
				frame(&ctx, &mut editor, &mut state, &mut avatars, width, vec![]);
				ctx.memory_mut(|memory| {
					memory.request_focus(egui::Id::unique("profile-display-name"))
				});
				frame(
					&ctx,
					&mut editor,
					&mut state,
					&mut avatars,
					width,
					vec![egui::Event::Text(" Edited".into())],
				);
				let draft = editor.draft.as_ref().unwrap().clone();
				assert!(draft.name.contains("Edited"));
				let (_, labels) = frame(&ctx, &mut editor, &mut state, &mut avatars, width, vec![]);
				let save = labels
					.iter()
					.find(|(text, _)| text == "Save changes")
					.unwrap()
					.1;
				assert!(save.left() >= 0.0 && save.right() <= width);
				let mut sent = vec![];
				for pressed in [true, false] {
					let pos = save.center();
					let (commands, _) = frame(
						&ctx,
						&mut editor,
						&mut state,
						&mut avatars,
						width,
						vec![
							egui::Event::PointerMoved(pos),
							egui::Event::PointerButton {
								pos,
								button: egui::PointerButton::Primary,
								pressed,
								modifiers: egui::Modifiers::NONE,
							},
						],
					);
					sent.extend(commands);
				}
				assert_eq!(sent.len(), 1);
				assert!(
					frame(&ctx, &mut editor, &mut state, &mut avatars, width, vec![])
						.0
						.is_empty()
				);
				let Command::EditProfile {
					user,
					request,
					changes: Some(changes),
					..
				} = sent.pop().unwrap()
				else {
					panic!("save expected")
				};
				assert_eq!(changes.global_name, Some(Some(draft.name.clone())));
				state.apply(client_core::Envelope {
					generation: state.generation,
					event: client_core::Event::ProfileEdited {
						user,
						request,
						result: Err(client_core::auth::Failure::Ambiguous),
					},
				});
				let (_, labels) = frame(&ctx, &mut editor, &mut state, &mut avatars, width, vec![]);
				assert!(editor.draft.as_ref() == Some(&draft));
				assert!(!state.can_save_own_profile());
				let cancel = labels
					.iter()
					.find(|(text, _)| text == "Cancel")
					.unwrap()
					.1
					.center();
				for pressed in [true, false] {
					assert!(
						frame(
							&ctx,
							&mut editor,
							&mut state,
							&mut avatars,
							width,
							vec![
								egui::Event::PointerMoved(cancel),
								egui::Event::PointerButton {
									pos: cancel,
									button: egui::PointerButton::Primary,
									pressed,
									modifiers: egui::Modifiers::NONE
								}
							]
						)
						.0
						.is_empty()
					);
				}
				assert!(
					editor.draft.as_ref()
						== Some(&Draft::from_profile(
							state.own_profile.data.as_ref().unwrap()
						))
				);
			}
		}
	}
}
