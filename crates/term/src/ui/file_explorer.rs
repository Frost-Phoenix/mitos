//! File explorer picker and its filesystem actions.

use std::{
    borrow::Cow,
    error::Error as _,
    path::{Path, PathBuf, MAIN_SEPARATOR_STR},
    sync::Arc,
};

use theme::{icons::ICONS, Theme};
use tui::text::{Line, Span};
use view::{
    editor::FileOperation,
    quicklist::{QuicklistEntry, QuicklistPosition, QuicklistTarget},
    Editor,
};

use crate::{
    compositor,
    keymap::component::PickerItemAction,
    ui::{
        directory_content, directory_content_cancellable, icon_span,
        overlay::Overlay,
        picker::{self, PickerCallbackResult},
        prompt::Movement,
        Picker, PickerColumn, Prompt, PromptEvent,
    },
};

type ExplorerItem = (PathBuf, bool);
type ExplorerData = (PathBuf, Arc<Theme>, bool);
type FileExplorer = Picker<ExplorerItem, ExplorerData>;

pub fn file_explorer(root: PathBuf, editor: &Editor) -> Result<FileExplorer, std::io::Error> {
    let options = directory_content(&root, &editor.config().file_explorer)?;

    let columns = [PickerColumn::new(
        "path",
        |(path, is_dir): &ExplorerItem, (_root, theme, show_icons): &ExplorerData| {
            let name = path.file_name();
            let is_open = name.is_none() && *is_dir;
            let name = name.map_or_else(|| Cow::Borrowed(".."), |name| name.to_string_lossy());
            let directory_style = theme.get("ui.text.directory");

            if *is_dir {
                let mut spans = Vec::with_capacity(2);
                if *show_icons {
                    let icons = ICONS.load();
                    if let Some(directory) = icons.fs().directory() {
                        spans.push(icon_span(directory.get_with_style_or_default(
                            &name,
                            is_open,
                            theme,
                            directory_style,
                        )));
                    }
                }
                spans.push(Span::styled(format!("{name}/"), directory_style));
                Line::from(spans).into()
            } else {
                let mut spans = Vec::with_capacity(2);
                if *show_icons {
                    let icons = ICONS.load();
                    if let Some(file) = icons.fs().file() {
                        spans.push(icon_span(file.get_with_style_or_default(path, theme)));
                    }
                }
                spans.push(Span::raw(name.into_owned()));
                Line::from(spans).into()
            }
        },
    )];
    let action_root = root.clone();
    let picker = Picker::new_with_callback_result(
        editor.redraw().clone(),
        columns,
        0,
        options,
        (root, Arc::new(editor.theme.clone()), editor.config().icons),
        move |cx, (path, is_dir): &ExplorerItem, action| {
            if *is_dir {
                let new_root = stdx::path::normalize(path);
                return match directory_content(&new_root, &cx.editor.config().file_explorer) {
                    Ok(options) => picker::PickerCallbackResult::Replace {
                        options,
                        editor_data: (
                            new_root,
                            Arc::new(cx.editor.theme.clone()),
                            cx.editor.config().icons,
                        ),
                    },
                    Err(err) => {
                        cx.editor.set_error(|| err.to_string());
                        picker::PickerCallbackResult::KeepOpen
                    }
                };
            }

            if let Err(err) = cx.editor.open(path, action) {
                cx.editor.set_error(|| {
                    if let Some(err) = err.source() {
                        format!("{}", err)
                    } else {
                        format!("unable to open \"{}\"", path.display())
                    }
                });
            }
            picker::PickerCallbackResult::Close
        },
    )
    .with_title("File Explorer")
    .with_preview(|_editor, (path, _is_dir)| Some((path.as_path().into(), None)))
    .with_quicklist(|_editor, (path, is_dir)| {
        if *is_dir {
            return None;
        }

        Some(QuicklistEntry {
            target: QuicklistTarget::Path(path.clone()),
            position: QuicklistPosition::None,
        })
    })
    .with_file_updates(
        editor,
        |data| &data.0,
        |_, editor, data, cancel| {
            let config = editor.config().file_explorer.clone();
            Box::pin(async move {
                Ok(tokio::task::spawn_blocking(move || {
                    directory_content_cancellable(&data.0, &config, || cancel.is_canceled())
                })
                .await??)
            })
        },
    )
    .with_item_key(|(path, _)| path.clone())
    .with_item_actions(
        |maps| &maps.file_explorer,
        move |cx, (path, is_dir), action| {
            handle_item_action(cx, path, *is_dir, action, &action_root)
        },
    );

    Ok(picker)
}

fn handle_item_action(
    cx: &mut compositor::Context,
    path: &Path,
    is_dir: bool,
    action: PickerItemAction,
    root: &Path,
) -> PickerCallbackResult<ExplorerItem, ExplorerData> {
    match action {
        PickerItemAction::Create => {
            let prefill = create_prefill(path, is_dir, root);
            let root = root.to_owned();
            open_prompt(
                cx,
                "Create path:".to_owned(),
                prefill,
                None,
                move |cx, input| {
                    create_path(cx, input, &root);
                },
            )
        }
        PickerItemAction::Delete => {
            if let Err(err) = source_path(path, root) {
                cx.editor.set_error(|| err);
                return PickerCallbackResult::KeepOpen;
            }
            let title = format!(
                "Delete \"{}\"? Type y to confirm:",
                display_path(path, root)
            );
            let path = path.to_owned();
            let root = root.to_owned();
            open_prompt(cx, title, String::new(), None, move |cx, input| {
                if input != "y" {
                    cx.editor.set_status("Delete canceled");
                    return;
                }
                delete_path(cx, &path, is_dir, &root);
            })
        }
        PickerItemAction::Move => {
            if let Err(err) = source_path(path, root) {
                cx.editor.set_error(|| err);
                return PickerCallbackResult::KeepOpen;
            }
            let prefill = path.display().to_string();
            let movement = cursor_before_extension(path, is_dir);
            let path = path.to_owned();
            let root = root.to_owned();
            open_prompt(
                cx,
                "Move to:".to_owned(),
                prefill,
                movement,
                move |cx, input| {
                    move_path(cx, input, &path, &root);
                },
            )
        }
        PickerItemAction::Rename => {
            if let Err(err) = source_path(path, root) {
                cx.editor.set_error(|| err);
                return PickerCallbackResult::KeepOpen;
            }
            let prefill = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let movement = cursor_before_extension(path, is_dir);
            let path = path.to_owned();
            let root = root.to_owned();
            open_prompt(
                cx,
                "Rename to:".to_owned(),
                prefill,
                movement,
                move |cx, input| {
                    rename_path(cx, input, &path, &root);
                },
            )
        }
        PickerItemAction::Copy => {
            if let Err(err) = source_path(path, root) {
                cx.editor.set_error(|| err);
                return PickerCallbackResult::KeepOpen;
            }
            let prefill = copy_prefill(path, is_dir, root);
            let path = path.to_owned();
            let root = root.to_owned();
            open_prompt(
                cx,
                "Copy to:".to_owned(),
                prefill,
                None,
                move |cx, input| {
                    copy_path(cx, input, &path, &root);
                },
            )
        }
        PickerItemAction::YankPath => {
            yank_path(cx, path);
            PickerCallbackResult::KeepOpen
        }
        PickerItemAction::CloseBuffer => PickerCallbackResult::KeepOpen,
    }
}

fn open_prompt(
    cx: &mut compositor::Context,
    title: String,
    prefill: String,
    movement: Option<Movement>,
    mut on_validate: impl FnMut(&mut compositor::Context, &str) + 'static,
) -> PickerCallbackResult<ExplorerItem, ExplorerData> {
    let mut prompt = Prompt::new_with_callback(
        Cow::Owned(title),
        None,
        crate::ui::completers::none,
        move |cx, input, event| {
            if event != PromptEvent::Validate {
                return None;
            }
            on_validate(cx, input);
            Some(refresh_explorer())
        },
    )
    .with_line(prefill, cx.editor);
    if let Some(movement) = movement {
        prompt.move_cursor(movement);
    }

    PickerCallbackResult::KeepOpenWithCallback(Box::new(move |compositor, _| {
        compositor.push(Box::new(prompt))
    }))
}

fn refresh_explorer() -> compositor::Callback {
    Box::new(|compositor, _| {
        if let Some(overlay) = compositor.find::<Overlay<FileExplorer>>() {
            overlay.content.request_file_refresh();
        } else if let Some(picker) = compositor.find::<FileExplorer>() {
            picker.request_file_refresh();
        }
    })
}

fn create_path(cx: &mut compositor::Context, input: &str, root: &Path) {
    if input.is_empty() {
        cx.editor.set_error(|| "Path cannot be empty");
        return;
    }

    let path = resolve_path(input, root);
    if !is_within_root(&path, root) || !parent_resolves_inside_root(&path, root) {
        cx.editor
            .set_error(|| "Create target must be inside the explorer root");
        return;
    }
    let is_dir = has_trailing_separator(input);
    let display = display_path(&path, root);
    match cx
        .editor
        .request_file_operation(FileOperation::Create { path, is_dir })
    {
        Ok(()) => cx.editor.set_status(format!("Created {display}")),
        Err(err) => cx
            .editor
            .set_error(|| format!("Could not create {display}: {err}")),
    }
}

fn move_path(cx: &mut compositor::Context, input: &str, path: &Path, root: &Path) {
    let source = match source_path(path, root) {
        Ok(path) => path,
        Err(err) => {
            cx.editor.set_error(|| err);
            return;
        }
    };
    if input.is_empty() {
        cx.editor.set_error(|| "Destination cannot be empty");
        return;
    }

    let mut destination = resolve_path(input, root);
    if destination.is_dir() {
        destination.push(source.file_name().unwrap());
    } else if has_trailing_separator(input) {
        cx.editor
            .set_error(|| "Destination directory does not exist");
        return;
    }
    if !destination_is_allowed(&destination, root) {
        cx.editor
            .set_error(|| "Destination resolves outside the explorer root");
        return;
    }
    move_to(cx, source, destination, root);
}

fn rename_path(cx: &mut compositor::Context, input: &str, path: &Path, root: &Path) {
    let source = match source_path(path, root) {
        Ok(path) => path,
        Err(err) => {
            cx.editor.set_error(|| err);
            return;
        }
    };
    if input.is_empty() {
        cx.editor.set_error(|| "New name cannot be empty");
        return;
    }

    let typed = Path::new(input);
    let mut destination =
        if typed.is_absolute() || input.contains(MAIN_SEPARATOR_STR) || input.contains('/') {
            resolve_path(input, root)
        } else {
            stdx::path::canonicalize(source.parent().unwrap_or(root).join(input))
        };
    if destination.is_dir() {
        destination.push(source.file_name().unwrap());
    }
    if !destination_is_allowed(&destination, root) {
        cx.editor
            .set_error(|| "Destination resolves outside the explorer root");
        return;
    }
    move_to(cx, source, destination, root);
}

fn move_to(cx: &mut compositor::Context, source: PathBuf, destination: PathBuf, root: &Path) {
    if source == destination {
        cx.editor.set_status("Path unchanged");
        return;
    }
    let source_display = display_path(&source, root);
    let destination_display = display_path(&destination, root);
    match cx.editor.request_file_operation(FileOperation::Move {
        from: source,
        to: destination,
    }) {
        Ok(()) => cx
            .editor
            .set_status(format!("Moved {source_display} to {destination_display}")),
        Err(err) => cx
            .editor
            .set_error(|| format!("Could not move {source_display}: {err}")),
    }
}

fn copy_path(cx: &mut compositor::Context, input: &str, path: &Path, root: &Path) {
    let source = match source_path(path, root) {
        Ok(path) => path,
        Err(err) => {
            cx.editor.set_error(|| err);
            return;
        }
    };
    if input.is_empty() {
        cx.editor.set_error(|| "Destination cannot be empty");
        return;
    }
    if source.is_dir() {
        cx.editor
            .set_error(|| "Only files can be copied from the explorer");
        return;
    }

    let mut destination = resolve_path(input, root);
    if destination.is_dir() {
        destination.push(source.file_name().unwrap());
    } else if has_trailing_separator(input) {
        cx.editor
            .set_error(|| "Destination directory does not exist");
        return;
    }
    if !destination_is_allowed(&destination, root) {
        cx.editor
            .set_error(|| "Destination resolves outside the explorer root");
        return;
    }
    let source_display = display_path(&source, root);
    let destination_display = display_path(&destination, root);
    match cx.editor.request_file_operation(FileOperation::Copy {
        from: source,
        to: destination,
    }) {
        Ok(()) => cx
            .editor
            .set_status(format!("Copied {source_display} to {destination_display}")),
        Err(err) => cx
            .editor
            .set_error(|| format!("Could not copy {source_display}: {err}")),
    }
}

fn delete_path(cx: &mut compositor::Context, path: &Path, is_dir: bool, root: &Path) {
    let path = match source_path(path, root) {
        Ok(path) => path,
        Err(err) => {
            cx.editor.set_error(|| err);
            return;
        }
    };
    let display = display_path(&path, root);
    match cx.editor.request_file_operation(FileOperation::Delete {
        path,
        recursive: is_dir,
    }) {
        Ok(()) => cx.editor.set_status(format!("Deleted {display}")),
        Err(err) => cx
            .editor
            .set_error(|| format!("Could not delete {display}: {err}")),
    }
}

fn yank_path(cx: &mut compositor::Context, path: &Path) {
    let register = cx
        .editor
        .selected_register
        .take()
        .unwrap_or(cx.editor.config().clipboard.default_yank_register);
    let path = stdx::path::get_relative_path(path)
        .to_string_lossy()
        .into_owned();
    match cx.editor.registers.write(register, vec![path]) {
        Ok(()) => cx
            .editor
            .set_status(format!("Yanked path to register \"{register}\"")),
        Err(err) => cx.editor.set_error(|| err.to_string()),
    }
}

fn source_path(path: &Path, root: &Path) -> Result<PathBuf, String> {
    if path.file_name().is_none() {
        return Err("The parent directory entry cannot be modified".to_owned());
    }
    let path = stdx::path::canonicalize(path);
    if !is_within_root(&path, root) {
        return Err("Selected path is outside the explorer root".to_owned());
    }
    if !parent_resolves_inside_root(&path, root) {
        return Err("Selected path resolves outside the explorer root".to_owned());
    }
    Ok(path)
}

fn resolve_path(input: &str, root: &Path) -> PathBuf {
    let path = stdx::path::expand_tilde(PathBuf::from(input)).into_owned();
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    stdx::path::canonicalize(path)
}

fn is_within_root(path: &Path, root: &Path) -> bool {
    let root = stdx::path::canonicalize(root);
    let path = stdx::path::canonicalize(path);
    path != root && path.starts_with(root)
}

/// Resolve symlinked parents so an entry below the root cannot reach outside it.
fn parent_resolves_inside_root(path: &Path, root: &Path) -> bool {
    let parent = path.parent().unwrap_or(path);
    let parent = stdx::path::canonicalize_existing(parent);
    let root = stdx::path::canonicalize_existing(root);
    parent.starts_with(root)
}

/// Paths typed inside the root may not escape through a symlinked parent.
/// Explicit destinations outside the root remain available.
fn destination_is_allowed(path: &Path, root: &Path) -> bool {
    !is_within_root(path, root) || parent_resolves_inside_root(path, root)
}

fn has_trailing_separator(input: &str) -> bool {
    input.ends_with(MAIN_SEPARATOR_STR) || input.ends_with('/')
}

fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn create_prefill(path: &Path, is_dir: bool, root: &Path) -> String {
    let directory = if is_dir {
        path
    } else {
        path.parent().unwrap_or(root)
    };
    format!("{}{}", directory.display(), MAIN_SEPARATOR_STR)
}

fn copy_prefill(path: &Path, is_dir: bool, root: &Path) -> String {
    let directory = path.parent().unwrap_or(root);
    let name = if is_dir {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        match (path.file_stem(), path.extension()) {
            (Some(stem), Some(extension)) => format!(
                "{} copy.{}",
                stem.to_string_lossy(),
                extension.to_string_lossy()
            ),
            (Some(stem), None) => format!("{} copy", stem.to_string_lossy()),
            _ => "copy".to_owned(),
        }
    };
    format!("{}{}{}", directory.display(), MAIN_SEPARATOR_STR, name)
}

fn cursor_before_extension(path: &Path, is_dir: bool) -> Option<Movement> {
    if is_dir {
        return None;
    }
    path.extension()
        .map(|extension| Movement::BackwardChar(extension.to_string_lossy().chars().count() + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_paths_must_be_strict_descendants_of_the_root() {
        let root = Path::new("/tmp/workspace");
        assert!(is_within_root(Path::new("/tmp/workspace/file.txt"), root));
        assert!(is_within_root(
            Path::new("/tmp/workspace/sub/file.txt"),
            root
        ));
        assert!(!is_within_root(Path::new("/tmp/workspace"), root));
        assert!(!is_within_root(Path::new("/tmp/workspace/.."), root));
        assert!(!is_within_root(Path::new("/tmp/other/file.txt"), root));
    }

    #[test]
    fn relative_destinations_resolve_from_the_explorer_root() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_path("sub/new.txt", root.path()),
            stdx::path::canonicalize(root.path().join("sub/new.txt"))
        );
    }

    #[test]
    fn the_parent_entry_has_no_file_name() {
        assert!(Path::new("/tmp/workspace/..").file_name().is_none());
        assert!(Path::new("/tmp/workspace/file.txt").file_name().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parents_cannot_escape_the_root() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("file.txt"), "outside").unwrap();
        symlink(&outside, root.join("link")).unwrap();

        let escaped = root.join("link/file.txt");
        assert!(is_within_root(&escaped, &root));
        assert!(!parent_resolves_inside_root(&escaped, &root));
        assert!(!destination_is_allowed(&escaped, &root));
        // The symlink itself can still be removed as a link.
        assert!(parent_resolves_inside_root(&root.join("link"), &root));
    }
}
