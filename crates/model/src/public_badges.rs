//! Public account badges the official client shows beside a name, derived from the
//! service-supplied account flags and Nitro tier.
//!
//! The service documents the user flag bits (`public_flags`); see
//! <https://docs.discord.com/developers/resources/user#user-object>. Only the bits that
//! carry a badge are listed here; every other bit is ignored. The badge order follows the
//! official catalog order. Never persisted as artwork.
use serde::{Deserialize, Serialize};

/// The official client shows at most this many public badges beside a name.
pub const MAX_PUBLIC_BADGES: usize = 6;

/// Documented user flag bits that carry a public badge.
pub mod flag {
	/// Discord employee.
	pub const STAFF: u32 = 1 << 0;
	/// Partnered server owner.
	pub const PARTNER: u32 = 1 << 1;
	/// HypeSquad events member.
	pub const HYPESQUAD: u32 = 1 << 2;
	/// Bug hunter, level 1.
	pub const BUG_HUNTER_LEVEL_1: u32 = 1 << 3;
	/// House Bravery member.
	pub const HOUSE_BRAVERY: u32 = 1 << 6;
	/// House Brilliance member.
	pub const HOUSE_BRILLIANCE: u32 = 1 << 7;
	/// House Balance member.
	pub const HOUSE_BALANCE: u32 = 1 << 8;
	/// Early Nitro supporter.
	pub const EARLY_SUPPORTER: u32 = 1 << 9;
	/// Bug hunter, level 2.
	pub const BUG_HUNTER_LEVEL_2: u32 = 1 << 14;
	/// Verified bot.
	pub const VERIFIED_BOT: u32 = 1 << 16;
	/// Early verified bot developer.
	pub const VERIFIED_DEVELOPER: u32 = 1 << 17;
	/// Moderator programs alumni.
	pub const CERTIFIED_MODERATOR: u32 = 1 << 18;
	/// Active developer.
	pub const ACTIVE_DEVELOPER: u32 = 1 << 22;
}

/// The service-supplied public account state behind the public badges.
///
/// `public_flags` carries the documented badge bits; `premium_type` is the Nitro tier,
/// where anything above zero means a subscription.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountFlags {
	/// The service's `public_flags`, truncated to the bits that fit.
	pub public_flags: u32,
	/// The service's Nitro tier; zero means no subscription.
	pub premium_type: u8,
}

impl AccountFlags {
	/// Builds the flags from the service values, ignoring anything out of range.
	pub fn new(public_flags: u64, premium_type: u8) -> Self {
		Self {
			public_flags: u32::try_from(public_flags).unwrap_or(u32::MAX),
			premium_type,
		}
	}
}

/// A public badge the client shows next to a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
	Staff,
	Partner,
	HypeSquad,
	BugHunterLevel1,
	HouseBravery,
	HouseBrilliance,
	HouseBalance,
	EarlySupporter,
	BugHunterLevel2,
	VerifiedBot,
	VerifiedDeveloper,
	CertifiedModerator,
	ActiveDeveloper,
	Nitro,
}

impl Kind {
	/// The catalog name the client uses for this badge.
	pub fn id(self) -> &'static str {
		match self {
			Self::Staff => "staff",
			Self::Partner => "partner",
			Self::HypeSquad => "hypesquad",
			Self::BugHunterLevel1 => "bug_hunter_level_1",
			Self::HouseBravery => "hypesquad_house_1",
			Self::HouseBrilliance => "hypesquad_house_2",
			Self::HouseBalance => "hypesquad_house_3",
			Self::EarlySupporter => "early_supporter",
			Self::BugHunterLevel2 => "bug_hunter_level_2",
			Self::VerifiedBot => "verified_bot",
			Self::VerifiedDeveloper => "verified_developer",
			Self::CertifiedModerator => "certified_moderator",
			Self::ActiveDeveloper => "active_developer",
			Self::Nitro => "premium",
		}
	}
	/// The flag bit this badge comes from; Nitro has none and comes from the tier.
	fn bit(self) -> u32 {
		match self {
			Self::Staff => flag::STAFF,
			Self::Partner => flag::PARTNER,
			Self::HypeSquad => flag::HYPESQUAD,
			Self::BugHunterLevel1 => flag::BUG_HUNTER_LEVEL_1,
			Self::HouseBravery => flag::HOUSE_BRAVERY,
			Self::HouseBrilliance => flag::HOUSE_BRILLIANCE,
			Self::HouseBalance => flag::HOUSE_BALANCE,
			Self::EarlySupporter => flag::EARLY_SUPPORTER,
			Self::BugHunterLevel2 => flag::BUG_HUNTER_LEVEL_2,
			Self::VerifiedBot => flag::VERIFIED_BOT,
			Self::VerifiedDeveloper => flag::VERIFIED_DEVELOPER,
			Self::CertifiedModerator => flag::CERTIFIED_MODERATOR,
			Self::ActiveDeveloper => flag::ACTIVE_DEVELOPER,
			Self::Nitro => 0,
		}
	}
	/// The order the official client lists the badges in; a subscription leads the row.
	fn order(self) -> u8 {
		match self {
			Self::Nitro => 0,
			Self::Staff => 1,
			Self::Partner => 2,
			Self::HypeSquad => 3,
			Self::BugHunterLevel1 => 4,
			Self::HouseBravery => 5,
			Self::HouseBrilliance => 6,
			Self::HouseBalance => 7,
			Self::EarlySupporter => 8,
			Self::BugHunterLevel2 => 9,
			Self::VerifiedBot => 10,
			Self::VerifiedDeveloper => 11,
			Self::CertifiedModerator => 12,
			Self::ActiveDeveloper => 13,
		}
	}
}

/// Every badge the flags carry, in the official order.
fn kinds_in_order(flags: u32) -> Vec<Kind> {
	[
		Kind::Staff,
		Kind::Partner,
		Kind::HypeSquad,
		Kind::BugHunterLevel1,
		Kind::HouseBravery,
		Kind::HouseBrilliance,
		Kind::HouseBalance,
		Kind::EarlySupporter,
		Kind::BugHunterLevel2,
		Kind::VerifiedBot,
		Kind::VerifiedDeveloper,
		Kind::CertifiedModerator,
		Kind::ActiveDeveloper,
	]
	.into_iter()
	.filter(|kind| flags & kind.bit() != 0)
	.collect()
}

/// The public badges a user shows beside their name.
///
/// A subscription leads the row, as in the official client, and the row is capped like
/// the client's.
pub fn badges(flags: &AccountFlags) -> Vec<Kind> {
	let mut shown = kinds_in_order(flags.public_flags);
	if flags.premium_type > 0 {
		shown.push(Kind::Nitro);
	}
	shown.sort_by_key(|kind| kind.order());
	shown.truncate(MAX_PUBLIC_BADGES);
	shown
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn documented_bits_map_to_badge_kinds() {
		assert_eq!(
			kinds_in_order(flag::STAFF | flag::ACTIVE_DEVELOPER),
			[Kind::Staff, Kind::ActiveDeveloper]
		);
		assert!(kinds_in_order(1 << 31).is_empty());
		assert_eq!(Kind::EarlySupporter.id(), "early_supporter");
		assert_eq!(Kind::Nitro.id(), "premium");
	}

	#[test]
	fn badges_follow_the_official_order_and_the_cap() {
		let flags = AccountFlags::new(
			u64::from(flag::ACTIVE_DEVELOPER | flag::STAFF | flag::EARLY_SUPPORTER),
			0,
		);
		assert_eq!(
			badges(&flags),
			[Kind::Staff, Kind::EarlySupporter, Kind::ActiveDeveloper]
		);
		// A subscription leads the row.
		let nitro = AccountFlags::new(u64::from(flag::STAFF), 2);
		assert_eq!(badges(&nitro)[0], Kind::Nitro);
		// The row never grows past the official cap.
		let all = AccountFlags::new(
			u64::from(
				flag::STAFF
					| flag::PARTNER | flag::HYPESQUAD
					| flag::BUG_HUNTER_LEVEL_1
					| flag::HOUSE_BRAVERY
					| flag::HOUSE_BRILLIANCE
					| flag::HOUSE_BALANCE
					| flag::EARLY_SUPPORTER
					| flag::BUG_HUNTER_LEVEL_2
					| flag::VERIFIED_BOT
					| flag::VERIFIED_DEVELOPER
					| flag::CERTIFIED_MODERATOR
					| flag::ACTIVE_DEVELOPER,
			),
			1,
		);
		assert_eq!(badges(&all).len(), MAX_PUBLIC_BADGES);
		// Out-of-range flags are ignored rather than rejected.
		assert!(badges(&AccountFlags::new(u64::MAX, 0)).len() <= MAX_PUBLIC_BADGES);
	}
}
