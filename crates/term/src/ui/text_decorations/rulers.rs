//! Glyph rulers repainted over whitespace after the text pass.

use text_layout::{doc_formatter::FormattedGrapheme, DisplayPosition};
use theme::Style;

use super::Decoration;
use crate::ui::document::{LinePos, TextRenderer};

/// Restores ruler glyphs on whitespace cells without covering document text.
///
/// Rulers are drawn before the document so text stays readable. Whitespace then
/// overwrites those glyphs, leaving gaps in the ruler. This decoration records
/// whitespace cells as they render and repaints the ruler glyphs once the line is
/// complete, including expanded tabs and line-ending cells.
pub struct RulerGlyph<'a> {
    columns: &'a [u16],
    character: char,
    style: Style,
    pending: Vec<(usize, usize)>,
}

impl<'a> RulerGlyph<'a> {
    pub fn new(columns: &'a [u16], character: char, style: Style) -> Self {
        Self {
            columns,
            character,
            style,
            pending: Vec::new(),
        }
    }
}

impl Decoration for RulerGlyph<'_> {
    fn reset_pos(&mut self, pos: usize) -> usize {
        self.pending.clear();
        pos
    }

    fn decorate_grapheme(
        &mut self,
        _renderer: &mut TextRenderer,
        grapheme: &FormattedGrapheme,
    ) -> usize {
        if grapheme.is_virtual() {
            return grapheme.char_idx;
        }

        let next = grapheme
            .char_idx
            .saturating_add(grapheme.doc_chars().max(1));
        if !grapheme.is_whitespace() {
            return next;
        }

        let start = grapheme.visual_pos.col;
        let end = start + grapheme.width();
        for &ruler in self.columns {
            let Some(col) = usize::from(ruler).checked_sub(1) else {
                continue;
            };
            if (start..end).contains(&col) {
                self.pending.push((grapheme.visual_pos.row, col));
            }
        }
        next
    }

    fn render_virt_lines(
        &mut self,
        renderer: &mut TextRenderer,
        _pos: LinePos,
        _virt_off: DisplayPosition,
    ) -> DisplayPosition {
        let style = self.style;
        let mut character_buffer = [0; 4];
        let character = self.character.encode_utf8(&mut character_buffer);
        for (row, col) in self.pending.drain(..) {
            renderer.overlay_grapheme(character, style, row, col);
        }
        DisplayPosition::default()
    }
}
