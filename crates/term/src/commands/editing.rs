//! Text transformations and editing commands over selections.

use super::{context::Context, mode::exit_select_mode};
use crate::ui::{self, Prompt, PromptEvent};
use editor_core::{line_ending::line_end_char_index, Range, Rope, Selection, Tendril, Transaction};
use stdx::rope::RopeExt;
use ui_core::{input::KeyEvent, keyboard::KeyCode};
use view::editing::surround::{self, Delimiter, Replacement};
use view::{document::Mode, info::Info, Editor};

pub(super) fn align_selections(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    let Some(transaction) = editor_core::transform::align_selections(
        doc.text(),
        doc.selection(view.id),
        doc.tab_width(),
    ) else {
        cx.editor
            .set_error(|| "align cannot work with multi line selections");
        return;
    };
    if let Err(error) = view::editing::apply(doc, &transaction, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

fn apply_transform(cx: &mut Context, operation: fn(&Rope, &Selection) -> Transaction) {
    let (view, doc) = current!(cx.editor);
    let transaction = operation(doc.text(), doc.selection(view.id));
    if let Err(error) = view::editing::apply(doc, &transaction, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

pub(super) fn switch_case(cx: &mut Context) {
    apply_transform(cx, editor_core::transform::switch_case);
}

pub(super) fn switch_to_uppercase(cx: &mut Context) {
    apply_transform(cx, editor_core::transform::switch_to_uppercase);
}

pub(super) fn switch_to_lowercase(cx: &mut Context) {
    apply_transform(cx, editor_core::transform::switch_to_lowercase);
}

pub(super) fn join_selections(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Err(error) = view::editing::join_selections(doc, view.id, false) {
        cx.editor.set_error(|| error.to_string());
    }
}

pub(super) fn join_selections_space(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Err(error) = view::editing::join_selections(doc, view.id, true) {
        cx.editor.set_error(|| error.to_string());
    }
}

fn reorder_selection_contents(cx: &mut Context, strategy: editor_core::transform::ReorderStrategy) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    if let Err(error) = view::editing::reorder_selection_contents(doc, view.id, strategy, count) {
        cx.editor.set_error(|| error.to_string());
    }
}

pub(super) fn rotate_selection_contents_forward(cx: &mut Context) {
    reorder_selection_contents(cx, editor_core::transform::ReorderStrategy::RotateForward);
}

pub(super) fn rotate_selection_contents_backward(cx: &mut Context) {
    reorder_selection_contents(cx, editor_core::transform::ReorderStrategy::RotateBackward);
}

pub(super) fn reverse_selection_contents(cx: &mut Context) {
    reorder_selection_contents(cx, editor_core::transform::ReorderStrategy::Reverse);
}

pub(super) fn indent(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let transaction =
        editor_core::transform::indent(doc.text(), doc.selection(view.id), doc.indent_style, count);
    if let Err(error) = view::editing::apply(doc, &transaction, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

pub(super) fn unindent(cx: &mut Context) {
    let count = cx.count();
    let (view, doc) = current!(cx.editor);
    let transaction = editor_core::transform::unindent(
        doc.text(),
        doc.selection(view.id),
        doc.tab_width(),
        count * doc.indent_width(),
    );
    if let Err(error) = view::editing::apply(doc, &transaction, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

pub(super) fn toggle_comments(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Err(error) = view::editing::toggle_comments(doc, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

pub(super) fn toggle_line_comments(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Err(error) = view::editing::toggle_line_comments(doc, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

pub(super) fn toggle_block_comments(cx: &mut Context) {
    let (view, doc) = current!(cx.editor);
    if let Err(error) = view::editing::toggle_block_comments(doc, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    exit_select_mode(cx);
}

pub(super) fn replace_char(cx: &mut Context) {
    let mut buf = [0u8; 4]; // To hold utf8 encoded char.

    // need to wait for next key
    cx.on_next_key(move |cx, event| {
        let (view, doc) = current!(cx.editor);
        let ch: Option<&str> = match event {
            KeyEvent {
                code: KeyCode::Char(ch),
                ..
            } => Some(ch.encode_utf8(&mut buf[..])),
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => Some(doc.line_ending.as_str()),
            KeyEvent {
                code: KeyCode::Tab, ..
            } => Some("\t"),
            _ => None,
        };

        let selection = doc.selection(view.id);

        if let Some(ch) = ch {
            let transaction = editor_core::transform::replace_graphemes(doc.text(), selection, ch);
            if let Err(error) = view::editing::apply(doc, &transaction, view.id) {
                cx.editor.set_error(|| error.to_string());
                return;
            }
            exit_select_mode(cx);
        }
    })
}

pub(super) fn replace(cx: &mut Context) {
    let prompt = Prompt::new_with_callback(
        "replace:".into(),
        None,
        ui::completers::none,
        |_cx, input, event| {
            if event != PromptEvent::Validate {
                return None;
            }
            let replacement = Tendril::from(input);
            Some(Box::new(move |compositor, cx| {
                if let Err(error) = replace_selections(cx.editor, &replacement) {
                    cx.editor.set_error(|| error.to_string());
                    return;
                }
                compositor
                    .find::<ui::EditorView>()
                    .unwrap()
                    .repeat
                    .replace(replacement);
            }))
        },
    );
    cx.push_layer(Box::new(prompt));
}

pub(crate) fn replace_selections(
    editor: &mut Editor,
    replacement: &str,
) -> Result<(), view::editing::EditError> {
    let scrolloff = editor.config().scroll.padding;
    editor.replace_selections(replacement)?;
    let (view, doc) = current!(editor);
    view.ensure_cursor_in_view(doc, scrolloff);
    if editor.mode() == Mode::Select {
        editor.set_mode(Mode::Normal);
    }
    Ok(())
}

enum Operation {
    Delete,
    Change,
}

enum YankAction {
    Yank,
    NoYank,
}

fn delete_selection_impl(cx: &mut Context, operation: Operation, yank: YankAction) {
    let register = matches!(yank, YankAction::Yank).then(|| {
        cx.register
            .unwrap_or(cx.editor.config().clipboard.default_yank_register)
    });
    let result = match operation {
        Operation::Change => cx.editor.change_selections(register, cx.count()),
        Operation::Delete => cx.editor.delete_selections(register),
    };
    if let Err(error) = result {
        cx.editor.set_error(|| error.to_string());
    }
}

pub(super) fn delete_selection(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Delete, YankAction::Yank);
}

pub(super) fn delete_selection_noyank(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Delete, YankAction::NoYank);
}

pub(super) fn change_selection(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Change, YankAction::Yank);
}

pub(super) fn change_selection_noyank(cx: &mut Context) {
    delete_selection_impl(cx, Operation::Change, YankAction::NoYank);
}

static SURROUND_HELP_TEXT: [(&str, &str); 6] = [
    ("m", "Nearest matching pair"),
    ("( or )", "Parentheses"),
    ("{ or }", "Curly braces"),
    ("< or >", "Angled brackets"),
    ("[ or ]", "Square brackets"),
    (" ", "... or any character"),
];

pub(super) fn surround_add(cx: &mut Context) {
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        let delimiter = match event.char() {
            Some(ch) => Delimiter::Character(ch),
            None if event.code == KeyCode::Enter => Delimiter::LineEnding,
            None => return,
        };
        let (view, doc) = current!(cx.editor);
        match surround::add(doc, view.id, delimiter) {
            Ok(()) => exit_select_mode(cx),
            Err(err) => cx.editor.set_error(|| err.to_string()),
        }
    });
    cx.editor.autoinfo = Some(Info::new(
        "Surround selections with",
        &SURROUND_HELP_TEXT[1..],
    ));
}

pub(super) fn surround_replace(cx: &mut Context) {
    let count = cx.count();
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        let character = match event.char() {
            Some('m') => None,
            Some(ch) => Some(ch),
            None => return,
        };
        let (view, doc) = current!(cx.editor);
        let replacement = match Replacement::prepare(doc, view.id, character, count) {
            Ok(replacement) => replacement,
            Err(err) => {
                cx.editor.set_error(|| err.to_string());
                return;
            }
        };
        cx.on_next_key(move |cx, event| {
            cx.editor.autoinfo = None;
            let (view, doc) = current!(cx.editor);
            let Some(character) = event.char() else {
                replacement.cancel(doc, view.id);
                return;
            };
            match replacement.apply(doc, view.id, character) {
                Ok(()) => exit_select_mode(cx),
                Err(err) => cx.editor.set_error(|| err.to_string()),
            }
        });
        cx.editor.autoinfo = Some(Info::new(
            "Replace with a pair of",
            &SURROUND_HELP_TEXT[1..],
        ));
    });
    cx.editor.autoinfo = Some(Info::new(
        "Replace surrounding pair of",
        &SURROUND_HELP_TEXT,
    ));
}

pub(super) fn surround_delete(cx: &mut Context) {
    let count = cx.count();
    cx.on_next_key(move |cx, event| {
        cx.editor.autoinfo = None;
        let character = match event.char() {
            Some('m') => None,
            Some(ch) => Some(ch),
            None => return,
        };
        let (view, doc) = current!(cx.editor);
        match surround::delete(doc, view.id, character, count) {
            Ok(()) => exit_select_mode(cx),
            Err(err) => cx.editor.set_error(|| err.to_string()),
        }
    });
    cx.editor.autoinfo = Some(Info::new("Delete surrounding pair of", &SURROUND_HELP_TEXT));
}

#[derive(Debug, PartialEq, Eq)]
pub enum MoveSelection {
    Below,
    Above,
}

#[derive(Clone)]
struct ExtendedChange {
    line_start: usize,
    line_end: usize,
    line_text: Option<Tendril>,
    line_selection: Option<(usize, usize)>,
}

/// Move line or block of text in specified direction.
/// The function respects single line, single selection, multiple lines using
/// several cursors and multiple selections.
fn move_selection(cx: &mut Context, direction: MoveSelection) {
    let (view, doc) = current!(cx.editor);
    let selection = doc.selection(view.id);
    let text = doc.text();
    let slice = text.slice(..);
    let mut last_step_changes: Vec<ExtendedChange> = vec![];
    let mut at_doc_edge = false;
    let all_changes = selection.into_iter().map(|range| {
        let (start, end) = range.line_range(slice);
        let line_start = text.line_to_char_idx(start);
        let line_end = line_end_char_index(&slice, end);
        let line_text = text.slice_chars(line_start..line_end).to_string();
        let next_line = match direction {
            MoveSelection::Above => start.saturating_sub(1),
            MoveSelection::Below => end + 1,
        };
        let rel_pos_anchor = range.anchor - line_start;
        let rel_pos_head = range.head - line_start;
        let cursor_rel_pos = (rel_pos_anchor, rel_pos_head);
        if next_line == start || next_line >= text.len_lines(stdx::rope::LINE_TYPE) || at_doc_edge {
            at_doc_edge = true;
            let changes = vec![ExtendedChange {
                line_start,
                line_end,
                line_text: Some(line_text.into()),
                line_selection: Some(cursor_rel_pos),
            }];
            last_step_changes = changes.clone();
            changes
        } else {
            let next_line_start = text.line_to_char_idx(next_line);
            let next_line_end = line_end_char_index(&slice, next_line);
            let next_line_text = text.slice_chars(next_line_start..next_line_end).to_string();
            let changes = match direction {
                MoveSelection::Above => vec![
                    ExtendedChange {
                        line_start: next_line_start,
                        line_end: next_line_end,
                        line_text: Some(line_text.into()),
                        line_selection: Some(cursor_rel_pos),
                    },
                    ExtendedChange {
                        line_start,
                        line_end,
                        line_text: Some(next_line_text.into()),
                        line_selection: None,
                    },
                ],
                MoveSelection::Below => vec![
                    ExtendedChange {
                        line_start,
                        line_end,
                        line_text: Some(next_line_text.into()),
                        line_selection: None,
                    },
                    ExtendedChange {
                        line_start: next_line_start,
                        line_end: next_line_end,
                        line_text: Some(line_text.into()),
                        line_selection: Some(cursor_rel_pos),
                    },
                ],
            };
            let changes = if last_step_changes.len() > 1 {
                evaluate_changes(last_step_changes.clone(), changes, &direction)
            } else {
                changes
            };
            last_step_changes = changes.clone();
            changes
        }
    });

    /// Merge changes from subsequent cursors
    fn evaluate_changes(
        mut last_changes: Vec<ExtendedChange>,
        current_changes: Vec<ExtendedChange>,
        direction: &MoveSelection,
    ) -> Vec<ExtendedChange> {
        let mut current_it = current_changes.into_iter();
        if let (Some(mut last), Some(mut current_first), Some(current_last)) =
            (last_changes.pop(), current_it.next(), current_it.next())
        {
            if last.line_start == current_first.line_start {
                match direction {
                    MoveSelection::Above => {
                        last.line_start = current_last.line_start;
                        last.line_end = current_last.line_end;
                        if let Some(first) = last_changes.pop() {
                            last_changes.push(first)
                        }
                        last_changes.extend([current_first, last]);
                        last_changes
                    }
                    MoveSelection::Below => {
                        current_first.line_start = last_changes[0].line_start;
                        current_first.line_end = last_changes[0].line_end;
                        last_changes[0] = current_first;
                        last_changes.extend([last, current_last]);
                        last_changes
                    }
                }
            } else {
                if let Some(first) = last_changes.pop() {
                    last_changes.push(first)
                }
                last_changes.extend([last, current_first, current_last]);
                last_changes
            }
        } else {
            last_changes
        }
    }
    let mut flattened: Vec<Vec<ExtendedChange>> = all_changes.into_iter().collect();
    let last_changes = flattened.pop().unwrap_or_default();
    let acc_cursors = get_adjusted_selection(doc, &last_changes, direction, at_doc_edge);
    let changes = last_changes
        .into_iter()
        .map(|change| (change.line_start, change.line_end, change.line_text));
    let new_sel = Selection::new(acc_cursors.into(), 0);
    let transaction = Transaction::change(doc.text(), changes);
    if let Err(error) = view::editing::apply(doc, &transaction, view.id) {
        cx.editor.set_error(|| error.to_string());
        return;
    }
    doc.set_selection(view.id, new_sel);
}

/// Returns selection range that is valid for the updated document
/// This logic is necessary because it's not possible to apply changes
/// to the document first and then set selection.
fn get_adjusted_selection(
    doc: &view::Document,
    last_changes: &[ExtendedChange],
    direction: MoveSelection,
    at_doc_edge: bool,
) -> Vec<Range> {
    let mut first_change_len = 0;
    let mut next_start = 0;
    let mut acc_cursors: Vec<Range> = vec![];

    for change in last_changes.iter() {
        let change_len = change.line_text.as_ref().map_or(0, |x| x.chars().count());
        if let Some((rel_anchor, rel_head)) = change.line_selection {
            let (anchor, head) = if at_doc_edge {
                let anchor = change.line_start + rel_anchor;
                let head = change.line_start + rel_head;
                (anchor, head)
            } else {
                match direction {
                    MoveSelection::Above => {
                        if next_start == 0 {
                            next_start = change.line_start;
                        }
                        let anchor = next_start + rel_anchor;
                        let head = next_start + rel_head;
                        // If there is next cursor below, selection position should be adjusted
                        // according to the length of the current line.
                        next_start += change_len + doc.line_ending.len_chars();
                        (anchor, head)
                    }
                    MoveSelection::Below => {
                        let anchor = change.line_start + first_change_len + rel_anchor - change_len;
                        let head = change.line_start + first_change_len + rel_head - change_len;
                        (anchor, head)
                    }
                }
            };
            let cursor = Range::new(anchor, head);
            if let Some(last) = acc_cursors.pop() {
                if cursor.overlaps(&last) {
                    acc_cursors.push(last);
                } else {
                    acc_cursors.push(last);
                    acc_cursors.push(cursor);
                };
            } else {
                acc_cursors.push(cursor);
            };
        } else {
            first_change_len = change.line_text.as_ref().map_or(0, |x| x.chars().count());
            next_start = 0;
        };
    }
    acc_cursors
}

pub(super) fn move_lines_up(cx: &mut Context) {
    move_selection(cx, MoveSelection::Above)
}

pub(super) fn move_lines_down(cx: &mut Context) {
    move_selection(cx, MoveSelection::Below)
}

enum IncrementDirection {
    Increase,
    Decrease,
}

/// Increment objects within selections by count.
pub(super) fn increment(cx: &mut Context) {
    increment_impl(cx, IncrementDirection::Increase);
}

/// Decrement objects within selections by count.
pub(super) fn decrement(cx: &mut Context) {
    increment_impl(cx, IncrementDirection::Decrease);
}

/// Increment objects within selections by `amount`.
/// A negative `amount` will decrement objects within selections.
fn increment_impl(cx: &mut Context, increment_direction: IncrementDirection) {
    let sign = match increment_direction {
        IncrementDirection::Increase => 1,
        IncrementDirection::Decrease => -1,
    };
    let amount = sign * i64::try_from(cx.count()).unwrap_or(i64::MAX);
    let step = if cx.register == Some('#') { sign } else { 0 };
    let (view, doc) = current!(cx.editor);
    let Some(transaction) = editor_core::transform::increment_selections(
        doc.text(),
        doc.selection(view.id),
        amount,
        step,
    ) else {
        return;
    };
    match view::editing::apply(doc, &transaction, view.id) {
        Ok(()) => exit_select_mode(cx),
        Err(error) => cx.editor.set_error(|| error.to_string()),
    }
}

pub(super) mod typed {
    //! Typable editing commands.

    use crate::{compositor, ui::PromptEvent};
    use ::command_line::Args;
    use anyhow::bail;

    pub(in crate::commands) fn sort(
        cx: &mut compositor::Context,
        args: Args,
        event: PromptEvent,
    ) -> anyhow::Result<()> {
        if event != PromptEvent::Validate {
            return Ok(());
        }

        let scrolloff = cx.editor.config().scroll.padding;
        let (view, doc) = current!(cx.editor);
        let Some(transaction) = editor_core::transform::sort_selection_contents(
            doc.text(),
            doc.selection(view.id),
            args.has_flag("insensitive"),
            args.has_flag("reverse"),
        ) else {
            bail!("Sorting requires multiple selections. Hint: split selection first");
        };
        view::editing::apply(doc, &transaction, view.id)?;
        doc.commit_changes();
        view.sync_changes(doc);
        view.ensure_cursor_in_view(doc, scrolloff);

        Ok(())
    }

    #[cold]
    pub(in crate::commands) fn reflow(
        cx: &mut compositor::Context,
        args: Args,
        event: PromptEvent,
    ) -> anyhow::Result<()> {
        if event != PromptEvent::Validate {
            return Ok(());
        }

        let scrolloff = cx.editor.config().scroll.padding;
        let (view, doc) = current!(cx.editor);

        // Find the text_width by checking the following sources in order:
        //   - The passed argument in `args`
        //   - The configured text-width for this language in languages.toml
        //   - The configured text-width in the config.toml
        let text_width: usize = args
            .first()
            .map(|num| num.parse::<usize>())
            .transpose()?
            .unwrap_or_else(|| doc.text_width());

        let transaction = editor_core::transform::reflow_selections(
            doc.text(),
            doc.selection(view.id),
            text_width,
            doc.line_ending,
        );
        view::editing::apply(doc, &transaction, view.id)?;
        doc.commit_changes();
        view.sync_changes(doc);
        view.ensure_cursor_in_view(doc, scrolloff);

        Ok(())
    }
}
