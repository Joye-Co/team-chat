//! Files as a Mac app treats them: Quick Look with Space, 共享…, 存储为… and dragging a
//! file out of the window to Finder or another app.
//!
//! A [`FileRef`] names a file attachment or a 产出. Its local copy comes from the provider
//! (`fetch_file` downloads an attachment into the cache, `open_artifact_version` puts a
//! 产出 version on disk); an action that needs it waits for it, and a drag offers the
//! file only once it is on disk (it starts the download, so the next drag has it).
//!
//! Space: a focused file item (a 产出 tile reached with Tab or the arrows) previews
//! itself; otherwise the file under the pointer does. Text fields keep their Space (GPUI
//! gives typed characters to the focused input before any key binding).

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{
    App, AppContext as _, AsyncApp, Context, ExternalDragPayload, FileDragPaths,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels, Point, Render,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, WindowId, div,
    px,
};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::{
    copy::{files as f, fill},
    data::{ArtifactFolder, ChatProvider, FileAttachment, FileError},
    ui::Sym,
};

gpui::actions!(files, [QuickLook]);

/// Space previews the focused or hovered file (call once at start-up). Bound without a
/// context, so any deeper Space binding (a pick list) wins.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("space", QuickLook, None)]);
    cx.on_action(|_: &QuickLook, cx| {
        let active = cx.active_window().map(|w| w.window_id());
        match hovered(active) {
            Some(target) => quick_look(&target, cx),
            // Nothing to preview: Space goes on to the focused control.
            None => cx.propagate(),
        }
    });
}

/// A file the person can preview, share, save or drag out.
#[derive(Clone)]
pub enum FileRef {
    Attachment {
        provider: Arc<dyn ChatProvider>,
        group_id: String,
        topic_id: String,
        file: FileAttachment,
    },
    Artifact {
        provider: Arc<dyn ChatProvider>,
        id: String,
        title: String,
    },
}

/// Where a file is on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    /// What Quick Look shows (a page's HTML file).
    pub preview: PathBuf,
    /// What is shared, saved or dragged: the file, or a page's folder.
    pub item: PathBuf,
    pub is_dir: bool,
}

impl FileRef {
    pub fn attachment(
        provider: &Arc<dyn ChatProvider>,
        group_id: &str,
        topic_id: &str,
        file: &FileAttachment,
    ) -> Self {
        Self::Attachment {
            provider: provider.clone(),
            group_id: group_id.into(),
            topic_id: topic_id.into(),
            file: file.clone(),
        }
    }

    pub fn artifact(provider: &Arc<dyn ChatProvider>, id: &str, title: &str) -> Self {
        Self::Artifact {
            provider: provider.clone(),
            id: id.into(),
            title: title.into(),
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Attachment { file, .. } => format!("file:{}:{}", file.sha256, file.name),
            Self::Artifact { id, .. } => format!("artifact:{id}"),
        }
    }

    /// The name the file goes by (the save panel suggests it).
    pub fn name(&self) -> String {
        match self {
            Self::Attachment { file, .. } => file.name.clone(),
            Self::Artifact { title, .. } => {
                let known = artifact_local(self).map(|l| l.item);
                match known {
                    Some(p) if p.is_file() => p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| export_name(title)),
                    _ => export_name(title),
                }
            }
        }
    }

    /// The local copy when it is on disk already. Cheap; never touches the network.
    pub fn local_now(&self) -> Option<Local> {
        match self {
            Self::Attachment { provider, file, .. } => provider.cached_file(file).map(|p| Local {
                preview: p.clone(),
                item: p,
                is_dir: false,
            }),
            Self::Artifact { .. } => artifact_local(self).filter(|l| l.item.exists()),
        }
    }

    /// Gets the local copy (downloads it when needed), then calls `then`. A failure shows
    /// the error alert.
    pub fn with_local(&self, cx: &mut App, then: impl FnOnce(Local, &mut App) + 'static) {
        if let Some(local) = self.local_now() {
            then(local, cx);
            return;
        }
        let name = self.name();
        let target = self.clone();
        match self {
            Self::Attachment {
                provider,
                group_id,
                topic_id,
                file,
            } => {
                let rx = provider.fetch_file(group_id, topic_id, file);
                cx.spawn(async move |cx: &mut AsyncApp| {
                    let result = rx.await.unwrap_or(Err(FileError::Other));
                    cx.update(|cx| match result {
                        Ok(p) => then(
                            Local {
                                preview: p.clone(),
                                item: p,
                                is_dir: false,
                            },
                            cx,
                        ),
                        Err(e) => failed(f::OPEN_FAILED, &name, e, cx),
                    })
                })
                .detach();
            }
            Self::Artifact {
                provider,
                id,
                title,
            } => {
                let rx = provider.open_artifact_version(id, None);
                let title = title.clone();
                cx.spawn(async move |cx: &mut AsyncApp| {
                    let folder = rx.await.unwrap_or(Err(FileError::Other));
                    // A page's folder is copied under its title once (off the main thread).
                    let local = match folder {
                        Ok(folder) => cx
                            .background_spawn(async move { stage_artifact(&folder, &title) })
                            .await
                            .ok_or(FileError::Other),
                        Err(e) => Err(e),
                    };
                    cx.update(|cx| match local {
                        Ok(local) => {
                            remember_artifact(&target, &local);
                            then(local, cx)
                        }
                        Err(e) => failed(f::OPEN_FAILED, &name, e, cx),
                    })
                })
                .detach();
            }
        }
    }

    /// Starts getting the local copy without doing anything with it (a drag starts it,
    /// so the file is on disk when the pointer leaves the window).
    fn prefetch(&self, cx: &mut App) {
        if self.local_now().is_none() {
            self.with_local(cx, |_, _| {});
        }
    }
}

fn failed(template: &str, name: &str, error: FileError, cx: &mut App) {
    super::notify_error(
        fill(
            template,
            &[("name", name), ("reason", super::error_text(error))],
        ),
        cx,
    );
}

// --- Quick Look -----------------------------------------------------------------------

/// Previews the file in the Quick Look panel (closes the panel when it is open).
pub fn quick_look(target: &FileRef, cx: &mut App) {
    target.with_local(cx, |local, _| {
        super::native::toggle_quick_look(local.preview)
    });
}

thread_local! {
    /// The file item under the pointer, with its window.
    static HOVERED: RefCell<Option<(String, WindowId, FileRef)>> = const { RefCell::new(None) };
    /// 产出 put on disk this session, by id.
    static ARTIFACTS: RefCell<HashMap<String, Local>> = RefCell::new(HashMap::new());
}

fn hovered(active: Option<WindowId>) -> Option<FileRef> {
    HOVERED.with_borrow(|h| {
        h.as_ref()
            .filter(|(_, window, _)| Some(*window) == active)
            .map(|(_, _, target)| target.clone())
    })
}

fn set_hovered(target: &FileRef, on: bool, window: WindowId) {
    let key = target.key();
    HOVERED.with_borrow_mut(|h| {
        if on {
            *h = Some((key, window, target.clone()));
        } else if h
            .as_ref()
            .is_some_and(|(k, w, _)| *k == key && *w == window)
        {
            *h = None;
        }
    });
}

fn artifact_local(target: &FileRef) -> Option<Local> {
    let FileRef::Artifact { id, .. } = target else {
        return None;
    };
    ARTIFACTS.with_borrow(|a| a.get(id).cloned())
}

fn remember_artifact(target: &FileRef, local: &Local) {
    if let FileRef::Artifact { id, .. } = target {
        ARTIFACTS.with_borrow_mut(|a| {
            a.insert(id.clone(), local.clone());
        });
    }
}

// --- 产出 on disk -----------------------------------------------------------------------

/// A file name made from a 产出 title: no path separators or colons, not empty.
pub fn export_name(title: &str) -> String {
    let cleaned: String = title
        .trim()
        .chars()
        .map(|c| match c {
            '/' | ':' | '\\' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    if cleaned.is_empty() {
        crate::copy::panel::ARTIFACTS.to_string()
    } else {
        cleaned
    }
}

/// Counts the files under `dir`, stopping after `limit`.
fn count_files(dir: &Path, limit: usize) -> usize {
    let mut n = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                n += 1;
                if n > limit {
                    return n;
                }
            }
        }
    }
    n
}

/// The local copy of a 产出 version: a single file as it is; a page of several files as a
/// copy of its folder named after the title (next to the version folder), so a drag or
/// a share carries the whole page under a readable name.
pub fn stage_artifact(folder: &ArtifactFolder, title: &str) -> Option<Local> {
    let entry = folder.dir.join(&folder.entry);
    if !folder.is_page || count_files(&folder.dir, 1) <= 1 {
        return entry.exists().then(|| Local {
            preview: entry.clone(),
            item: entry,
            is_dir: false,
        });
    }
    let parent = folder.dir.parent()?;
    let staged = parent
        .join("export")
        .join(folder.version.to_string())
        .join(export_name(title));
    if !staged.is_dir() {
        copy_dir(&folder.dir, &staged).ok()?;
    }
    Some(Local {
        preview: staged.join(&folder.entry),
        item: staged,
        is_dir: true,
    })
}

/// Copies a folder and everything in it.
pub fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), &target)?;
        }
    }
    Ok(())
}

// --- 存储为… --------------------------------------------------------------------------

/// Asks where to save the file (the system save panel, in ~/Downloads first) and copies
/// it there.
pub fn save_as(target: &FileRef, cx: &mut App) {
    let name = target.name();
    target.with_local(cx, move |local, cx| {
        let dir = std::env::var_os("HOME")
            .map(|h| Path::new(&h).join("Downloads"))
            .unwrap_or_else(std::env::temp_dir);
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |cx: &mut AsyncApp| {
            let Ok(Ok(Some(dest))) = rx.await else {
                return;
            };
            let copied = cx
                .background_spawn(async move { copy_to(&local, &dest) })
                .await;
            if copied.is_err() {
                cx.update(|cx| failed(f::SAVE_FAILED, &name, FileError::Unreadable, cx));
            }
        })
        .detach();
    });
}

/// Copies the local file (or page folder) to `dest`, replacing what the save panel agreed
/// to replace.
fn copy_to(local: &Local, dest: &Path) -> std::io::Result<()> {
    if local.item == dest {
        return Ok(());
    }
    if local.is_dir {
        if dest.is_dir() {
            std::fs::remove_dir_all(dest)?;
        }
        copy_dir(&local.item, dest)
    } else {
        std::fs::copy(&local.item, dest).map(|_| ())
    }
}

// --- 共享… ----------------------------------------------------------------------------

/// Opens the share picker for the file at `at` (the point the menu opened from).
pub fn share(target: &FileRef, at: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    target.with_local(cx, move |local, cx| {
        let _ = handle.update(cx, |_, window, _| {
            super::native::share(vec![local.item], at, window)
        });
    });
}

// --- menus ----------------------------------------------------------------------------

/// 快速查看, 共享… and 存储为… at the end of a file's context menu. `at` is where the menu
/// opened (the share picker shows there).
pub fn menu_items(menu: PopupMenu, target: &FileRef, at: Point<Pixels>) -> PopupMenu {
    let (look, share_it, save) = (target.clone(), target.clone(), target.clone());
    menu.separator()
        .item(PopupMenuItem::new(f::QUICK_LOOK).on_click(move |_, _, cx| quick_look(&look, cx)))
        .item(
            // No symbol: the other items of this menu have none (NATIVE.md §7.3).
            PopupMenuItem::new(f::SHARE)
                .on_click(move |_, window, cx| share(&share_it, at, window, cx)),
        )
        .item(PopupMenuItem::new(f::SAVE_AS).on_click(move |_, _, cx| save_as(&save, cx)))
}

// --- the element ----------------------------------------------------------------------

/// What a file drag carries inside the window.
pub struct FileDrag {
    target: FileRef,
    name: SharedString,
    icon: Sym,
}

/// The chip under the pointer while a file drags inside the window (outside it, the
/// system draws the file's icon).
struct DragGhost {
    name: SharedString,
    icon: Sym,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::native::{NativeType as _, native_colors, tokens};
        let n = native_colors(cx);
        let k = crate::theme::colors(cx);
        div()
            .h(tokens::height::SM)
            .max_w(px(280.))
            .px(tokens::space::S8)
            .flex()
            .items_center()
            .gap(tokens::space::S6)
            .rounded(tokens::radius::ITEM)
            .border_1()
            .border_color(n.separator)
            .bg(n.content)
            .shadow(k.lift())
            .child(crate::ui::sym(
                self.icon,
                crate::ui::SymSize::INLINE,
                n.secondary,
            ))
            .child(
                crate::ui::truncate::file_name(self.name.clone())
                    .id("file-drag-name")
                    .n_subheadline()
                    .text_color(n.label),
            )
    }
}

/// A file item (row, tile, image): Space previews it while it is focused or under the
/// pointer, and it drags out of the window as the file.
pub trait FileItem: Sized {
    fn file_item(self, target: FileRef, icon: Sym) -> Self;
}

impl<E> FileItem for Stateful<E>
where
    Stateful<E>: gpui::StatefulInteractiveElement + gpui::InteractiveElement,
    E: gpui::Element,
{
    fn file_item(self, target: FileRef, icon: Sym) -> Self {
        let (hover, look) = (target.clone(), target.clone());
        let drag = FileDrag {
            name: target.name().into(),
            target,
            icon,
        };
        self.on_hover(move |on, window, _| {
            set_hovered(&hover, *on, window.window_handle().window_id())
        })
        .on_action(move |_: &QuickLook, _, cx| quick_look(&look, cx))
        .on_drag(drag, |d: &FileDrag, _, _, cx| {
            d.target.prefetch(cx);
            let (name, icon) = (d.name.clone(), d.icon);
            cx.new(|_| DragGhost { name, icon })
        })
        .external_drag_payload(|d: &FileDrag, _, _| {
            let local = d.target.local_now()?;
            Some(ExternalDragPayload::Files(FileDragPaths::new([(
                local.item,
                local.is_dir,
            )])))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_become_file_names() {
        assert_eq!(export_name("第三季度运营复盘"), "第三季度运营复盘");
        assert_eq!(export_name("  a/b:c\\d  "), "a-b-c-d");
        assert_eq!(export_name("..hidden"), "hidden");
        assert_eq!(export_name("   "), crate::copy::panel::ARTIFACTS);
        assert_eq!(export_name("line\nbreak"), "line break");
    }

    fn folder(dir: &Path, entry: &str, is_page: bool) -> ArtifactFolder {
        ArtifactFolder {
            artifact_id: "a".into(),
            version: 3,
            dir: dir.to_path_buf(),
            entry: entry.into(),
            is_page,
        }
    }

    #[test]
    fn a_single_file_is_used_as_it_is() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("3");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.pdf"), b"%PDF").unwrap();
        let local = stage_artifact(&folder(&dir, "report.pdf", false), "复盘").unwrap();
        assert_eq!(local.item, dir.join("report.pdf"));
        assert_eq!(local.preview, local.item);
        assert!(!local.is_dir);
        // A page of one file needs no folder either.
        std::fs::remove_file(dir.join("report.pdf")).unwrap();
        std::fs::write(dir.join("index.html"), b"<p>").unwrap();
        let local = stage_artifact(&folder(&dir, "index.html", true), "复盘").unwrap();
        assert_eq!(local.item, dir.join("index.html"));
    }

    #[test]
    fn a_page_of_several_files_is_staged_under_its_title() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("3");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), b"<p>").unwrap();
        std::fs::write(dir.join("assets").join("app.css"), b"p{}").unwrap();
        let local = stage_artifact(&folder(&dir, "index.html", true), "Q3: 复盘").unwrap();
        let staged = root
            .path()
            .join("a")
            .join("export")
            .join("3")
            .join("Q3- 复盘");
        assert_eq!(local.item, staged);
        assert!(local.is_dir);
        assert_eq!(local.preview, staged.join("index.html"));
        assert!(staged.join("assets").join("app.css").is_file());
        // Saving copies the whole folder.
        let dest = root.path().join("saved");
        copy_to(&local, &dest).unwrap();
        assert!(dest.join("index.html").is_file());
        assert!(dest.join("assets").join("app.css").is_file());
    }

    #[test]
    fn a_missing_entry_has_no_local_copy() {
        let root = tempfile::tempdir().unwrap();
        assert!(stage_artifact(&folder(root.path(), "gone.pdf", false), "x").is_none());
    }

    #[test]
    fn the_hovered_file_belongs_to_its_window() {
        let provider: Arc<dyn ChatProvider> = crate::fake::FakeProvider::new();
        let file = FileAttachment {
            name: "a.pdf".into(),
            sha256: "1".repeat(64),
            ..Default::default()
        };
        let target = FileRef::attachment(&provider, "g", "t", &file);
        let (w1, w2) = (WindowId::from(1), WindowId::from(2));
        set_hovered(&target, true, w1);
        assert!(hovered(Some(w1)).is_some());
        assert!(hovered(Some(w2)).is_none());
        assert!(hovered(None).is_none());
        // Leaving another window does not clear it; leaving its own does.
        set_hovered(&target, false, w2);
        assert!(hovered(Some(w1)).is_some());
        set_hovered(&target, false, w1);
        assert!(hovered(Some(w1)).is_none());
    }
}
