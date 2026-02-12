use crate::domain::config::Bookmark;

pub fn dedup_bookmark(bookmarks: &[Bookmark], path: &str, side: &str) -> bool {
    bookmarks
        .iter()
        .any(|b| b.path == path && b.side == side)
}
