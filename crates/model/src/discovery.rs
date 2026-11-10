//! One bounded, session-only page of discoverable servers; never a global search index.
use crate::Id;
use std::collections::BTreeSet;

pub const PAGE_SIZE: u32 = 24;
pub const MAX_GUILDS: usize = 48;
pub const MAX_CATEGORIES: usize = 64;
pub const MAX_BYTES: usize = 192 * 1024;
pub const MAX_NAME_BYTES: usize = 400;
pub const MAX_DESCRIPTION_BYTES: usize = 2048;
pub const MAX_CATEGORY_NAME_BYTES: usize = 128;
pub const MAX_CODE_BYTES: usize = 100;
pub const MAX_HASH_BYTES: usize = 34;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guild {
	pub id: Id,
	pub name: String,
	pub description: Option<String>,
	pub icon: Option<String>,
	/// Only this service-supplied code may enter the invite preview flow.
	pub vanity_url_code: Option<String>,
	pub approximate_member_count: u32,
	pub approximate_presence_count: u32,
	pub primary_category_id: u32,
	pub verified: bool,
	pub partnered: bool,
}

impl Guild {
	pub fn bytes(&self) -> usize {
		size_of::<Self>()
			+ self.name.capacity()
			+ self.description.as_ref().map_or(0, String::capacity)
			+ self.icon.as_ref().map_or(0, String::capacity)
			+ self.vanity_url_code.as_ref().map_or(0, String::capacity)
	}

	pub fn valid(&self) -> bool {
		self.id.0 != 0
			&& !self.name.trim().is_empty()
			&& text_valid(&self.name, MAX_NAME_BYTES, false)
			&& self
				.description
				.as_ref()
				.is_none_or(|description| text_valid(description, MAX_DESCRIPTION_BYTES, true))
			&& self
				.icon
				.as_ref()
				.is_none_or(|hash| hash.len() <= MAX_HASH_BYTES && crate::valid_avatar_hash(hash))
			&& self.vanity_url_code.as_ref().is_none_or(|code| {
				code.len() <= MAX_CODE_BYTES && crate::server_invites::valid_code(code)
			})
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
	/// Zero is a valid category, not a missing identifier.
	pub id: u32,
	pub name: String,
	pub is_primary: bool,
}

impl Category {
	pub fn bytes(&self) -> usize {
		size_of::<Self>() + self.name.capacity()
	}

	pub fn valid(&self) -> bool {
		!self.name.trim().is_empty() && text_valid(&self.name, MAX_CATEGORY_NAME_BYTES, false)
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
	pub guilds: Vec<Guild>,
	pub categories: Vec<Category>,
	pub total: u32,
	pub offset: u32,
	pub limit: u32,
}

impl Page {
	pub fn bytes(&self) -> usize {
		size_of::<Self>()
			+ self.guilds.iter().map(Guild::bytes).sum::<usize>()
			+ (self.guilds.capacity() - self.guilds.len()) * size_of::<Guild>()
			+ self.categories.iter().map(Category::bytes).sum::<usize>()
			+ (self.categories.capacity() - self.categories.len()) * size_of::<Category>()
	}

	pub fn valid(&self) -> bool {
		if !(1..=MAX_GUILDS as u32).contains(&self.limit)
			|| self.guilds.len() > self.limit as usize
			|| self.categories.len() > MAX_CATEGORIES
			|| self.offset.checked_add(self.limit).is_none()
			|| (!self.guilds.is_empty()
				&& self.offset.saturating_add(self.guilds.len() as u32) > self.total)
			|| self.bytes() > MAX_BYTES
		{
			return false;
		}
		let mut guilds = BTreeSet::new();
		let mut categories = BTreeSet::new();
		self.guilds
			.iter()
			.all(|guild| guild.valid() && guilds.insert(guild.id))
			&& self
				.categories
				.iter()
				.all(|category| category.valid() && categories.insert(category.id))
	}

	/// The returned cursor, not the number of locally filtered cards, drives pagination.
	pub fn next_offset(&self) -> Option<u32> {
		let next = self.offset.checked_add(self.limit)?;
		(self.limit != 0 && !self.guilds.is_empty() && next < self.total).then_some(next)
	}

	pub fn previous_offset(&self) -> Option<u32> {
		(self.offset != 0 && self.limit != 0).then(|| self.offset.saturating_sub(self.limit))
	}
}

fn text_valid(text: &str, max: usize, multiline: bool) -> bool {
	text.len() <= max
		&& !text
			.chars()
			.any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cursors_use_the_service_limit_and_empty_pages_stop() {
		let mut page = Page {
			guilds: vec![Guild {
				id: Id(1),
				name: "Synthetic".into(),
				description: None,
				icon: None,
				vanity_url_code: None,
				approximate_member_count: 5,
				approximate_presence_count: 1,
				primary_category_id: 0,
				verified: false,
				partnered: false,
			}],
			categories: vec![Category {
				id: 0,
				name: "All".into(),
				is_primary: true,
			}],
			total: 49,
			offset: 24,
			limit: 24,
		};
		assert!(page.valid());
		assert_eq!(page.next_offset(), Some(48));
		assert_eq!(page.previous_offset(), Some(0));
		page.offset = 48;
		assert_eq!(page.next_offset(), None);
		page.guilds.clear();
		page.total = 0;
		assert!(page.valid()); // A shrinking directory can leave a past-the-end cursor.
		assert_eq!(page.next_offset(), None);
		assert_eq!(page.previous_offset(), Some(24));
		page.offset = u32::MAX;
		assert!(!page.valid());
	}

	#[test]
	fn retained_allocations_are_included_in_the_budget() {
		let page = Page {
			guilds: Vec::with_capacity(MAX_BYTES / size_of::<Guild>() + 1),
			categories: vec![],
			total: 0,
			offset: 0,
			limit: PAGE_SIZE,
		};
		assert!(!page.valid());
	}
}
