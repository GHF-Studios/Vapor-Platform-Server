use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 200;
const MAX_CURSOR_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageCursor {
    pub(crate) created_at_unix: i64,
    pub(crate) id: String,
}

impl PageCursor {
    pub(crate) fn encode(&self) -> Result<String, ()> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| ())?;
        let encoded = URL_SAFE_NO_PAD.encode(bytes);
        (encoded.len() <= MAX_CURSOR_BYTES)
            .then_some(encoded)
            .ok_or(())
    }

    pub(crate) fn decode(encoded: &str) -> Result<Self, ()> {
        if encoded.is_empty() || encoded.len() > MAX_CURSOR_BYTES {
            return Err(());
        }
        let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| ())?;
        let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| ())?;
        cursor.validate()?;
        Ok(cursor)
    }

    fn validate(&self) -> Result<(), ()> {
        if self.created_at_unix < 0
            || self.id.is_empty()
            || self.id.len() > 128
            || self.id.chars().any(char::is_control)
        {
            return Err(());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

impl PageQuery {
    pub(crate) fn page_size(&self) -> usize {
        self.limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    pub(crate) fn decoded_cursor(&self) -> Result<Option<PageCursor>, ()> {
        self.cursor.as_deref().map(PageCursor::decode).transpose()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Page<T> {
    pub(crate) items: Vec<T>,
    pub(crate) next_cursor: Option<String>,
}

impl<T> Page<T> {
    pub(crate) fn from_items(
        mut items: Vec<T>,
        page_size: usize,
        cursor_for: impl FnOnce(&T) -> PageCursor,
    ) -> Result<Self, ()> {
        let has_more = items.len() > page_size;
        if has_more {
            items.pop();
        }
        let next_cursor = if has_more {
            items
                .last()
                .map(cursor_for)
                .map(|cursor| cursor.encode())
                .transpose()?
        } else {
            None
        };
        Ok(Self { items, next_cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::{Page, PageCursor, PageQuery};

    #[test]
    fn cursor_round_trip_preserves_the_stable_keyset() {
        let cursor = PageCursor {
            created_at_unix: 1_800_000_000,
            id: "0b9e5e6a-16e0-4dc1-a7e2-060fb6457289".to_owned(),
        };

        let encoded = cursor.encode().unwrap();

        assert_eq!(PageCursor::decode(&encoded).unwrap(), cursor);
    }

    #[test]
    fn cursor_rejects_malformed_or_unbounded_values() {
        assert!(PageCursor::decode("not-a-cursor").is_err());
        assert!(PageCursor::decode(&"a".repeat(2049)).is_err());
        assert!(PageCursor {
            created_at_unix: -1,
            id: "request-1".to_owned(),
        }
        .encode()
        .is_err());
        assert!(PageCursor {
            created_at_unix: 1,
            id: "bad\nid".to_owned(),
        }
        .encode()
        .is_err());
    }

    #[test]
    fn page_size_is_bounded_and_defaults_when_omitted() {
        assert_eq!(PageQuery::default().page_size(), 50);
        assert_eq!(
            PageQuery {
                limit: Some(0),
                cursor: None
            }
            .page_size(),
            1
        );
        assert_eq!(
            PageQuery {
                limit: Some(999),
                cursor: None
            }
            .page_size(),
            200
        );
    }

    #[test]
    fn page_emits_a_cursor_only_when_another_item_exists() {
        let rows = vec![(3, "c"), (2, "b"), (1, "a")];
        let page = Page::from_items(rows, 2, |row| PageCursor {
            created_at_unix: row.0,
            id: row.1.to_owned(),
        })
        .unwrap();

        assert_eq!(page.items, vec![(3, "c"), (2, "b")]);
        assert_eq!(
            PageCursor::decode(page.next_cursor.as_deref().unwrap()).unwrap(),
            PageCursor {
                created_at_unix: 2,
                id: "b".to_owned()
            }
        );

        let final_page = Page::from_items(vec![(1, "a")], 2, |row| PageCursor {
            created_at_unix: row.0,
            id: row.1.to_owned(),
        })
        .unwrap();
        assert!(final_page.next_cursor.is_none());
    }
}
