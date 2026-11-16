//! A Markdown 产出 read where it is opened: the 产出 pane beside a 话题 and the 产出 page
//! draw the document themselves ([`super::full::markdown_reader`]) instead of a card that
//! sends it to the viewer sheet. Other kinds keep that card.

use gpui::{AnyElement, App, IntoElement as _, Pixels, Styled as _, div};

use super::{
    MediaKind, file_kind, full, mime_for,
    store::{self, Data, Load, MediaKey},
};
use crate::data::ArtifactFolder;

/// Whether a version folder is a Markdown document the app reads in place.
pub fn reads_in_place(folder: &ArtifactFolder) -> bool {
    !folder.is_page && file_kind(&folder.entry, mime_for(&folder.entry)) == MediaKind::Markdown
}

/// The document of a Markdown version, read once from its folder on disk (the viewer sheet
/// shares the read). An empty frame while it is read; `None` when the folder is not
/// Markdown or its text cannot be read (too big, not UTF-8), so the caller shows its card
/// with 打开.
pub fn markdown(
    id: &str,
    folder: &ArtifactFolder,
    title: &str,
    measure: Pixels,
    pad: Pixels,
    cx: &mut App,
) -> Option<AnyElement> {
    if !reads_in_place(folder) {
        return None;
    }
    let key = MediaKey::new(folder.artifact_id.clone(), folder.version);
    if store::get(&key, cx).is_none() {
        store::request_folder(&key, title, folder.dir.clone(), folder.entry.clone(), cx);
    }
    match store::get(&key, cx)? {
        Load::Ready(media) => match &media.data {
            // Keyed by the version, so another document opens at its top.
            Data::Markdown(md) => Some(
                full::markdown_reader(
                    format!("{id}-{}-{}", folder.artifact_id, folder.version),
                    &media,
                    md,
                    measure,
                    pad,
                    cx,
                )
                .into_any_element(),
            ),
            _ => None,
        },
        // A local read: the frame stays empty for the moment it takes, like the folder
        // before it.
        Load::Loading => Some(div().size_full().into_any_element()),
        Load::Failed(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::reads_in_place;
    use crate::data::ArtifactFolder;

    fn folder(entry: &str, is_page: bool) -> ArtifactFolder {
        ArtifactFolder {
            artifact_id: "a".into(),
            version: 1,
            dir: std::env::temp_dir(),
            entry: entry.into(),
            is_page,
        }
    }

    #[test]
    fn only_markdown_files_read_in_place() {
        for entry in ["复盘.md", "docs/README.markdown", "NOTES.MD"] {
            assert!(reads_in_place(&folder(entry, false)), "{entry}");
        }
        for (entry, page) in [
            ("index.html", true),
            ("report.html", false),
            ("预算.pdf", false),
            ("data.csv", false),
            ("a.zip", false),
        ] {
            assert!(!reads_in_place(&folder(entry, page)), "{entry}");
        }
    }
}
