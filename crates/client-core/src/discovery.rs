//! Discoverable servers. One bounded page in memory; the directory itself stays remote.
//!
//! Ordinary browsing returns verified and partnered servers, not the whole directory, so the
//! UI labels local filtering as a filter of this page rather than a search.

use crate::auth::Failure;
use model::{
	Id,
	discovery::{Category, Guild, MAX_CATEGORIES, PAGE_SIZE, Page},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
	Load {
		category: Option<u32>,
		offset: u32,
		request: u64,
	},
}

#[derive(Clone, Debug)]
pub enum Event {
	Page {
		request: u64,
		result: Result<Page, Failure>,
	},
}

#[derive(Default)]
pub struct Discovery {
	page: Option<Page>,
	/// The query this page belongs to; a page for another filter is not shown under this one.
	loaded: Option<(Option<u32>, u32)>,
	category: Option<u32>,
	/// The page the service was asked for, which its answer echoes.
	offset: u32,
	loading: bool,
	request: u64,
	error: Option<&'static str>,
	/// Local filter for the rows already loaded.
	pub filter: String,
}

impl Discovery {
	pub fn page(&self) -> Option<&Page> {
		self.page.as_ref()
	}
	pub fn category(&self) -> Option<u32> {
		self.category
	}
	pub fn loading(&self) -> bool {
		self.loading
	}
	pub fn error(&self) -> Option<&'static str> {
		self.error
	}
	pub fn categories(&self) -> &[Category] {
		self.page
			.as_ref()
			.map_or(&[], |page| page.categories.as_slice())
	}
	/// Rows on this page that match the local filter, and how many did before filtering.
	pub fn guilds(&self) -> Vec<&Guild> {
		self.filtered().0
	}
	pub fn matched(&self) -> (usize, usize) {
		let (rows, total) = self.filtered();
		(rows.len(), total)
	}
	fn filtered(&self) -> (Vec<&Guild>, usize) {
		let Some(page) = self.page.as_ref() else {
			return (Vec::new(), 0);
		};
		let query = self.filter.trim().to_lowercase();
		let total = page.guilds.len();
		if query.is_empty() {
			return (page.guilds.iter().collect(), total);
		}
		(
			page.guilds
				.iter()
				.filter(|guild| {
					guild.name.to_lowercase().contains(&query)
						|| guild
							.description
							.as_ref()
							.is_some_and(|text| text.to_lowercase().contains(&query))
				})
				.collect(),
			total,
		)
	}
	pub fn next(&self) -> Option<u32> {
		self.page.as_ref()?.next_offset()
	}
	pub fn previous(&self) -> Option<u32> {
		self.page.as_ref()?.previous_offset()
	}
	pub fn joined(&self, state: &crate::State, guild: Id) -> bool {
		state.guild(guild).is_some()
	}
	pub fn next_request(&mut self) -> u64 {
		self.request = self.request.wrapping_add(1);
		self.request
	}
	pub fn begin(&mut self, category: Option<u32>, offset: u32, request: u64) {
		self.loading = true;
		self.request = request;
		self.category = category;
		self.offset = offset;
		self.error = None;
	}
	pub fn apply(&mut self, event: Event) {
		let Event::Page { request, result } = event;
		if request != self.request || !self.loading {
			return;
		}
		self.loading = false;
		match result {
			Ok(mut page) => {
				// Categories are fetched with the first page and carried across paging.
				let mut categories = std::mem::take(&mut page.categories);
				categories.truncate(MAX_CATEGORIES);
				if categories.is_empty()
					&& let Some(previous) = &self.page
				{
					categories = previous.categories.clone();
				}
				page.categories = categories;
				self.loaded = Some((self.category, self.offset));
				self.page = Some(page);
				self.error = None;
			}
			Err(failure) => self.error = Some(failure.label()),
		}
	}
	pub fn clear(&mut self) {
		*self = Self {
			request: self.request,
			..Default::default()
		};
	}
}

impl crate::State {
	/// Reads one page, unless the same page is already loading.
	pub fn request_discovery(
		&mut self,
		category: Option<u32>,
		offset: u32,
	) -> Option<crate::Command> {
		if !offset.is_multiple_of(PAGE_SIZE) {
			return None;
		}
		if self.discovery.loaded == Some((category, offset)) && !self.discovery.loading {
			return None;
		}
		let request = self.discovery.next_request();
		self.discovery.begin(category, offset, request);
		Some(crate::Command::Discovery(Command::Load {
			category,
			offset,
			request,
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn page(offset: u32, total: u32, categories: Vec<Category>) -> Page {
		Page {
			guilds: vec![Guild {
				id: Id(1),
				name: "Example".into(),
				description: Some("Games".into()),
				icon: None,
				vanity_url_code: None,
				approximate_member_count: 10,
				approximate_presence_count: 1,
				primary_category_id: 0,
				verified: false,
				partnered: false,
			}],
			categories,
			total,
			offset,
			limit: PAGE_SIZE,
		}
	}

	#[test]
	fn a_stale_page_cannot_replace_a_newer_filter() {
		let mut discovery = Discovery::default();
		discovery.begin(None, 0, 1);
		discovery.begin(Some(3), 0, 2);
		discovery.apply(Event::Page {
			request: 1,
			result: Ok(page(0, 1, Vec::new())),
		});
		assert!(discovery.page().is_none());
		discovery.apply(Event::Page {
			request: 2,
			result: Ok(page(
				0,
				1,
				vec![Category {
					id: 3,
					name: "Games".into(),
					is_primary: true,
				}],
			)),
		});
		assert_eq!(discovery.category(), Some(3));
		assert_eq!(discovery.categories().len(), 1);
	}

	#[test]
	fn pages_carry_categories_and_local_filtering_keeps_the_cursor() {
		let mut discovery = Discovery::default();
		discovery.begin(None, 0, 1);
		discovery.apply(Event::Page {
			request: 1,
			result: Ok(page(
				0,
				50,
				vec![Category {
					id: 0,
					name: "All".into(),
					is_primary: true,
				}],
			)),
		});
		assert_eq!(discovery.next(), Some(PAGE_SIZE));
		discovery.begin(None, PAGE_SIZE, 2);
		discovery.apply(Event::Page {
			request: 2,
			result: Ok(page(PAGE_SIZE, 50, Vec::new())),
		});
		assert_eq!(discovery.categories().len(), 1);
		discovery.filter = "missing".into();
		assert!(discovery.guilds().is_empty());
		assert_eq!(discovery.next(), None);
	}

	#[test]
	fn offsets_must_be_page_aligned() {
		let mut state = crate::State::default();
		assert!(state.request_discovery(None, 1).is_none());
		assert!(state.request_discovery(None, PAGE_SIZE).is_some());
		assert!(state.request_discovery(None, PAGE_SIZE).is_none());
	}
}
