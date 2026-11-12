//! Discoverable servers. Community-documented user routes; never a global search index.
use crate::DecodeError;
use model::{
	Id,
	discovery::{Category, Guild, MAX_CATEGORIES, MAX_GUILDS, Page},
};
use serde::Deserialize;

pub const MAX_DISCOVERY_WIRE: usize = 512 * 1024;
pub const MAX_CATEGORIES_WIRE: usize = 64 * 1024;

#[derive(Deserialize)]
struct GuildDto {
	id: Id,
	name: String,
	#[serde(default)]
	description: Option<String>,
	#[serde(default)]
	icon: Option<String>,
	#[serde(default)]
	vanity_url_code: Option<String>,
	#[serde(default)]
	approximate_member_count: Option<u32>,
	#[serde(default)]
	approximate_presence_count: Option<u32>,
	#[serde(default)]
	primary_category_id: Option<u32>,
	#[serde(default)]
	features: FeatureList,
}

/// Only the two badges this surface renders; every other feature is ignored.
#[derive(Default)]
struct FeatureList {
	verified: bool,
	partnered: bool,
}
impl<'de> Deserialize<'de> for FeatureList {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		struct Features;
		impl<'de> serde::de::Visitor<'de> for Features {
			type Value = FeatureList;
			fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
				f.write_str("a feature list")
			}
			fn visit_seq<A: serde::de::SeqAccess<'de>>(
				self,
				mut seq: A,
			) -> Result<Self::Value, A::Error> {
				let mut features = FeatureList::default();
				let mut count = 0;
				while let Some(value) = seq.next_element::<String>()? {
					count += 1;
					if count > 256 || value.len() > 128 {
						return Err(serde::de::Error::custom("Feature capacity"));
					}
					match value.as_str() {
						"VERIFIED" => features.verified = true,
						"PARTNERED" => features.partnered = true,
						_ => {}
					}
				}
				Ok(features)
			}
		}
		d.deserialize_seq(Features)
	}
}

fn guild(dto: GuildDto) -> Option<Guild> {
	let guild = Guild {
		id: dto.id,
		name: dto.name,
		description: dto.description.filter(|text| !text.trim().is_empty()),
		icon: dto.icon.filter(|hash| !hash.is_empty()),
		vanity_url_code: dto.vanity_url_code.filter(|code| !code.is_empty()),
		approximate_member_count: dto.approximate_member_count.unwrap_or(0),
		approximate_presence_count: dto.approximate_presence_count.unwrap_or(0),
		primary_category_id: dto.primary_category_id.unwrap_or(0),
		verified: dto.features.verified,
		partnered: dto.features.partnered,
	};
	guild.valid().then_some(guild)
}

#[derive(Deserialize)]
struct PageDto {
	total: u32,
	limit: u32,
	#[serde(default)]
	offset: u32,
}

/// `GET /discoverable-guilds` answers with a wrapper. A bare array is not that shape.
pub fn decode_page(bytes: &[u8], categories: Vec<Category>) -> Result<Page, DecodeError> {
	if bytes.len() > MAX_DISCOVERY_WIRE {
		return Err(DecodeError);
	}
	let page: PageDto = crate::decode(bytes)?;
	// Re-read the raw guilds through a bounded list so one unsupported row cannot fail the page.
	let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| DecodeError)?;
	let items = raw
		.get("guilds")
		.and_then(serde_json::Value::as_array)
		.ok_or(DecodeError)?;
	if items.len() > MAX_GUILDS {
		return Err(DecodeError);
	}
	let guilds = items
		.iter()
		.filter_map(|item| serde_json::from_value(item.clone()).ok())
		.filter_map(guild)
		.collect();
	let page = Page {
		guilds,
		categories,
		total: page.total,
		offset: page.offset,
		limit: page.limit,
	};
	page.valid().then_some(page).ok_or(DecodeError)
}

#[derive(Deserialize)]
struct CategoryDto {
	id: u32,
	name: String,
	#[serde(default)]
	is_primary: bool,
}

/// `GET /discovery/categories` answers with a bare array of `{id, name, is_primary}`.
pub fn decode_categories(bytes: &[u8]) -> Result<Vec<Category>, DecodeError> {
	if bytes.len() > MAX_CATEGORIES_WIRE {
		return Err(DecodeError);
	}
	let rows: Vec<CategoryDto> = crate::decode(bytes)?;
	if rows.len() > MAX_CATEGORIES {
		return Err(DecodeError);
	}
	let mut seen = std::collections::BTreeSet::new();
	let categories: Vec<_> = rows
		.into_iter()
		.map(|row| Category {
			id: row.id,
			name: row.name,
			is_primary: row.is_primary,
		})
		.filter(|category| category.valid() && seen.insert(category.id))
		.collect();
	Ok(categories)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_wrapper_page_keeps_supported_rows_and_drops_the_rest() {
		let json = r#"{"guilds":[
			{"id":"1","name":"Supported","description":"Hello","icon":"a_0123456789abcdef0123456789abcdef","vanity_url_code":"chat",
			 "approximate_member_count":400,"approximate_presence_count":20,"primary_category_id":5,
			 "features":["VERIFIED","COMMUNITY"]},
			{"id":"2","name":"","icon":"../../evil"},
			{"id":"0","name":"Zero"}
		],"total":40,"limit":24,"offset":0}"#;
		let page = decode_page(json.as_bytes(), Vec::new()).unwrap();
		assert_eq!(page.guilds.len(), 1);
		assert_eq!(
			page.guilds[0].icon.as_deref(),
			Some("a_0123456789abcdef0123456789abcdef")
		);
		assert!(page.guilds[0].verified && !page.guilds[0].partnered);
		assert_eq!(page.guilds[0].vanity_url_code.as_deref(), Some("chat"));
		assert_eq!(page.next_offset(), Some(24));
		assert!(
			decode_page(
				json.replace(r#""guilds":"#, r#""servers":"#).as_bytes(),
				Vec::new()
			)
			.is_err()
		);
		assert!(decode_page(json.replace(r#""total":40,"#, "").as_bytes(), Vec::new()).is_err());
		let invalid_hash = json.replace("a_0123456789abcdef0123456789abcdef", "a_12");
		assert!(
			decode_page(invalid_hash.as_bytes(), Vec::new())
				.unwrap()
				.guilds
				.is_empty()
		);
		let extra = json.replacen("\"guilds\":[", "\"guilds\":[null,", 1);
		assert_eq!(
			decode_page(extra.as_bytes(), Vec::new())
				.unwrap()
				.guilds
				.len(),
			1
		);
	}

	#[test]
	fn an_out_of_range_limit_and_oversized_payloads_are_refused() {
		let json = r#"{"guilds":[],"total":0,"limit":49,"offset":0}"#;
		assert!(decode_page(json.as_bytes(), Vec::new()).is_err());
		assert!(decode_page(b"[]", Vec::new()).is_err());
		let bytes = vec![b' '; MAX_DISCOVERY_WIRE + 1];
		assert!(decode_page(&bytes, Vec::new()).is_err());
		assert!(decode_categories(&bytes).is_err());
	}

	#[test]
	fn categories_keep_zero_and_collapse_duplicates() {
		let json = br#"[{"id":0,"name":"All","is_primary":true},{"id":0,"name":"Again"},{"id":3,"name":"Games","is_primary":true},{"id":4,"name":"  "}]"#;
		let categories = decode_categories(json).unwrap();
		assert_eq!(categories.len(), 2);
		assert!(categories[0].is_primary);
		assert_eq!(categories[1].id, 3);
	}
}
