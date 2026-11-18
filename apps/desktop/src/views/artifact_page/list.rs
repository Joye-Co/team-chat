//! The 产出 list's own model: what kind a row is (a web page, a PDF, a video, a
//! table, …), the filter of the toolbar (类型, 群, 做的人) and the view settings
//! kept on this computer (图标 or 列表, the order, rows per page).

use std::collections::BTreeSet;

use crate::{
    copy::group_page as gc,
    data::{ArtifactKind, ArtifactSummary, ChatProvider},
    ui::Sym,
    views::media::{self, MediaKind},
};

/// What a row of the 产出 list is: the media kinds of `views/media` (an image
/// set is 图片), and 决定, which is not a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ListKind {
    Page,
    Markdown,
    Image,
    Video,
    Audio,
    Pdf,
    Code,
    Table,
    Decision,
    File,
}

impl ListKind {
    pub fn label(self) -> &'static str {
        match self {
            ListKind::Decision => gc::GROUP_PAGE_KIND_DECISION,
            // The column is a word wide: a Markdown file is a 文档 here.
            ListKind::Markdown => crate::copy::artifact_page::ARTIFACTS_KIND_DOC,
            other => other.media().label(),
        }
    }

    pub fn sym(self) -> Sym {
        self.media().icon()
    }

    fn media(self) -> MediaKind {
        match self {
            ListKind::Page => MediaKind::Html,
            ListKind::Markdown | ListKind::Decision => MediaKind::Markdown,
            ListKind::Image => MediaKind::Image,
            ListKind::Video => MediaKind::Video,
            ListKind::Audio => MediaKind::Audio,
            ListKind::Pdf => MediaKind::Pdf,
            ListKind::Code => MediaKind::Code,
            ListKind::Table => MediaKind::Table,
            ListKind::File => MediaKind::File,
        }
    }

    fn of_media(kind: MediaKind) -> Self {
        match kind {
            MediaKind::Html => ListKind::Page,
            MediaKind::Markdown => ListKind::Markdown,
            MediaKind::Image | MediaKind::ImageSet => ListKind::Image,
            MediaKind::Video => ListKind::Video,
            MediaKind::Audio => ListKind::Audio,
            MediaKind::Pdf => ListKind::Pdf,
            MediaKind::Code => ListKind::Code,
            MediaKind::Table => ListKind::Table,
            MediaKind::File => ListKind::File,
        }
    }

    /// The kind of a 产出 from what this client already holds, without asking
    /// the server for anything: the sender's media metadata, else the entry of
    /// its version when the 产出 was opened or shown before, else its name.
    pub fn of(provider: &dyn ChatProvider, a: &ArtifactSummary) -> Self {
        if a.kind == ArtifactKind::Decision {
            return ListKind::Decision;
        }
        let sent = provider
            .artifact_media(&a.id, a.version)
            .and_then(|m| media::poster::local_kind(m.kind));
        let cached = || {
            let detail = provider.artifact(&a.id)?;
            let v = detail.versions.iter().find(|v| v.version == a.version)?;
            if v.entry.is_empty() {
                return None;
            }
            let files: Vec<media::MediaFile> = v
                .files
                .iter()
                .map(|f| media::MediaFile {
                    path: f.path.clone(),
                    size: f.size,
                    mime: f.mime.clone(),
                })
                .collect();
            Some(media::classify(&v.entry, &files))
        };
        Self::resolve(a.kind, &a.title, sent.or_else(cached))
    }

    /// [`Self::of`] from its parts: `known` is the media kind the client holds.
    pub fn resolve(kind: ArtifactKind, title: &str, known: Option<MediaKind>) -> Self {
        match (known, kind) {
            (_, ArtifactKind::Decision) => ListKind::Decision,
            (Some(known), _) => Self::of_media(known),
            (None, ArtifactKind::Page) => ListKind::Page,
            (None, ArtifactKind::Image) => ListKind::Image,
            // A file is often named after itself (`10 月渠道.csv`).
            (None, ArtifactKind::File) => match media::file_kind(title, "") {
                // A file 产出 is not a live page, whatever its name ends in.
                MediaKind::Html => ListKind::File,
                named => Self::of_media(named),
            },
        }
    }
}

/// The filter of the list: a row shows when it passes every dimension that has
/// a choice (an empty dimension lets everything through).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub kinds: BTreeSet<ListKind>,
    pub groups: BTreeSet<String>,
    pub makers: BTreeSet<String>,
}

impl ListFilter {
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.groups.is_empty() && self.makers.is_empty()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn passes(&self, kind: ListKind, group_id: &str, maker_id: &str) -> bool {
        (self.kinds.is_empty() || self.kinds.contains(&kind))
            && (self.groups.is_empty() || self.groups.contains(group_id))
            && (self.makers.is_empty() || self.makers.contains(maker_id))
    }

    pub fn toggle_kind(&mut self, kind: ListKind) {
        if !self.kinds.remove(&kind) {
            self.kinds.insert(kind);
        }
    }

    pub fn toggle_group(&mut self, id: &str) {
        if !self.groups.remove(id) {
            self.groups.insert(id.to_string());
        }
    }

    pub fn toggle_maker(&mut self, id: &str) {
        if !self.makers.remove(id) {
            self.makers.insert(id.to_string());
        }
    }
}

/// The view settings of the list in `layout.json` (key `artifacts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListPrefs {
    pub grid: bool,
    /// The order's slug (`time`, `name`, `kind`).
    pub sort: String,
    pub per_page: usize,
}

const KEY: &str = "artifacts";

impl ListPrefs {
    /// From the `artifacts` value of the layout file. Anything missing or of
    /// another type is `None` for that part, so the caller keeps its default.
    pub fn parse(value: &serde_json::Value) -> (Option<bool>, Option<String>, Option<usize>) {
        (
            value.get("grid").and_then(|v| v.as_bool()),
            value
                .get("sort")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            value
                .get("per_page")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize),
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "grid": self.grid, "sort": self.sort, "per_page": self.per_page })
    }

    /// What the person chose last time. Nothing in a self-test or a scene.
    pub fn load() -> (Option<bool>, Option<String>, Option<usize>) {
        if !crate::views::settings::prefs::saving() {
            return (None, None, None);
        }
        std::fs::read(crate::views::columns::layout_file())
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get(KEY).cloned())
            .map_or((None, None, None), |v| Self::parse(&v))
    }

    /// Keeps the choice beside the other layout keys. Never in a self-test.
    pub fn save(&self) {
        if crate::views::settings::prefs::saving() {
            crate::views::shell::layout::merge(&[(KEY, self.to_json())]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_named_by_what_the_client_knows_then_by_its_name() {
        use ArtifactKind as K;
        // The sender's metadata or the cached entry wins.
        assert_eq!(
            ListKind::resolve(K::File, "报告", Some(MediaKind::Pdf)),
            ListKind::Pdf
        );
        assert_eq!(
            ListKind::resolve(K::File, "九宫格", Some(MediaKind::ImageSet)),
            ListKind::Image
        );
        // Else the name of a file says it.
        assert_eq!(
            ListKind::resolve(K::File, "10 月渠道.csv", None),
            ListKind::Table
        );
        assert_eq!(
            ListKind::resolve(K::File, "录屏.mov", None),
            ListKind::Video
        );
        assert_eq!(
            ListKind::resolve(K::File, "复盘.md", None),
            ListKind::Markdown
        );
        assert_eq!(
            ListKind::resolve(K::File, "reconnect.ts", None),
            ListKind::Code
        );
        assert_eq!(
            ListKind::resolve(K::File, "安装包.dmg", None),
            ListKind::File
        );
        assert_eq!(
            ListKind::resolve(K::File, "导出.html", None),
            ListKind::File
        );
        assert_eq!(ListKind::resolve(K::Page, "月报", None), ListKind::Page);
        assert_eq!(
            ListKind::resolve(K::Decision, "定价.pdf", Some(MediaKind::Pdf)),
            ListKind::Decision
        );
        assert_ne!(ListKind::Pdf.label(), ListKind::Page.label());
        assert_ne!(ListKind::Video.sym(), ListKind::Table.sym());
    }

    #[test]
    fn the_filter_needs_every_chosen_dimension() {
        let mut f = ListFilter::default();
        assert!(f.is_empty());
        assert!(f.passes(ListKind::Pdf, "g1", "m1"));
        f.toggle_kind(ListKind::Pdf);
        f.toggle_kind(ListKind::Video);
        assert!(f.passes(ListKind::Video, "g1", "m1"));
        assert!(!f.passes(ListKind::Page, "g1", "m1"));
        f.toggle_group("g2");
        assert!(!f.passes(ListKind::Pdf, "g1", "m1"));
        assert!(f.passes(ListKind::Pdf, "g2", "m1"));
        f.toggle_maker("m9");
        assert!(!f.passes(ListKind::Pdf, "g2", "m1"));
        f.toggle_maker("m9");
        f.toggle_group("g2");
        f.toggle_kind(ListKind::Pdf);
        assert!(f.passes(ListKind::Video, "g1", "m1"));
        f.clear();
        assert!(f.is_empty());
    }

    #[test]
    fn view_settings_round_trip_and_ignore_what_they_do_not_know() {
        let prefs = ListPrefs {
            grid: true,
            sort: "kind".into(),
            per_page: 50,
        };
        assert_eq!(
            ListPrefs::parse(&prefs.to_json()),
            (Some(true), Some("kind".into()), Some(50))
        );
        let odd = serde_json::json!({ "grid": "yes", "per_page": 20 });
        assert_eq!(ListPrefs::parse(&odd), (None, None, Some(20)));
    }
}
