//! Public account badges shown beside a name, derived from the service's account flags.
//!
//! The official client draws these next to usernames in the member list, beside message
//! authors and on the profile card. The artwork is not bundled, so each badge is a small
//! colored emblem with the badge name as its hover text.
use crate::{design, icons, icons::Icon};
use egui::{Color32, Sense, Vec2};
use model::public_badges::Kind;

/// The badge emblem and its colour.
fn style(kind: Kind) -> (Icon, Color32) {
	match kind {
		Kind::Staff => (Icon::Verified, Color32::from_rgb(0x58, 0x7a, 0xf4)),
		Kind::Partner => (Icon::Crown, Color32::from_rgb(0xff, 0xc7, 0x3d)),
		Kind::HypeSquad => (Icon::Fire, Color32::from_rgb(0xff, 0x73, 0xfa)),
		Kind::BugHunterLevel1 | Kind::BugHunterLevel2 => {
			(Icon::ShieldWarning, Color32::from_rgb(0xf2, 0x6d, 0x5b))
		}
		Kind::HouseBravery | Kind::HouseBrilliance | Kind::HouseBalance => {
			(Icon::Sparkle, Color32::from_rgb(0xff, 0x8a, 0x4c))
		}
		Kind::EarlySupporter => (Icon::Star, Color32::from_rgb(0xff, 0xc7, 0x3d)),
		Kind::VerifiedBot => (Icon::Verified, Color32::from_rgb(0x58, 0x7a, 0xf4)),
		Kind::VerifiedDeveloper => (Icon::Verified, Color32::from_rgb(0x3a, 0xd1, 0x62)),
		Kind::CertifiedModerator => (Icon::ShieldWarning, Color32::from_rgb(0x3a, 0xd1, 0x62)),
		Kind::ActiveDeveloper => (Icon::Sparkle, Color32::from_rgb(0x3a, 0xd1, 0x62)),
		Kind::Nitro => (Icon::Sparkle, Color32::from_rgb(0xff, 0x73, 0xfa)),
	}
}

/// The locale key naming this badge.
fn label_key(kind: Kind) -> &'static str {
	match kind {
		Kind::Staff => "public-badge-staff",
		Kind::Partner => "public-badge-partner",
		Kind::HypeSquad => "public-badge-hypesquad",
		Kind::BugHunterLevel1 => "public-badge-bug-hunter-1",
		Kind::HouseBravery => "public-badge-house-bravery",
		Kind::HouseBrilliance => "public-badge-house-brilliance",
		Kind::HouseBalance => "public-badge-house-balance",
		Kind::EarlySupporter => "public-badge-early-supporter",
		Kind::BugHunterLevel2 => "public-badge-bug-hunter-2",
		Kind::VerifiedBot => "public-badge-verified-bot",
		Kind::VerifiedDeveloper => "public-badge-verified-developer",
		Kind::CertifiedModerator => "public-badge-certified-moderator",
		Kind::ActiveDeveloper => "public-badge-active-developer",
		Kind::Nitro => "public-badge-nitro",
	}
}

/// Draws one public badge emblem and returns its response for hover handling.
pub fn badge_icon(ui: &mut egui::Ui, kind: Kind, size: f32) -> egui::Response {
	let (icon, color) = style(kind);
	let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
	if ui.is_rect_visible(rect) {
		let colors = design::palette(ui);
		ui.painter()
			.circle_filled(rect.center(), size * 0.45, color);
		icons::paint(ui.painter(), icon, rect.shrink(size * 0.18), colors.raised);
	}
	response.on_hover_text(crate::i18n::translate(label_key(kind)))
}

/// Draws the whole public badge row of a user.
pub fn badge_row(ui: &mut egui::Ui, flags: &model::AccountFlags, size: f32) {
	for kind in model::public_badges::badges(flags) {
		badge_icon(ui, kind, size);
	}
}
