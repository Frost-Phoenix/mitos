//! Editor and component keymaps, including buffer-picker inheritance.
use crate::keymap::component::{
    BlameAction, ComponentKeymaps, MenuAction, PageAction, PickerAction, PickerItemAction,
    PopupAction, PromptAction,
};
use crate::keymap::{self, KeyTrie};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use ui_core::keymap::Keymap;
use view::document::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BufferPickerAction(PickerAction);

impl<'de> Deserialize<'de> for BufferPickerAction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::IntoDeserializer;
        let action = String::deserialize(deserializer)?;
        let action = match action.as_str() {
            "close_buffer" => PickerAction::Item(PickerItemAction::CloseBuffer),
            action => PickerAction::deserialize(action.into_deserializer())?,
        };
        Ok(Self(action))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileExplorerAction(PickerAction);

impl<'de> Deserialize<'de> for FileExplorerAction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::IntoDeserializer;
        let action = String::deserialize(deserializer)?;
        let action = match action.as_str() {
            "create" => PickerAction::Item(PickerItemAction::Create),
            "delete" => PickerAction::Item(PickerItemAction::Delete),
            "move" => PickerAction::Item(PickerItemAction::Move),
            "rename" => PickerAction::Item(PickerItemAction::Rename),
            "copy" => PickerAction::Item(PickerItemAction::Copy),
            "yank_path" => PickerAction::Item(PickerItemAction::YankPath),
            action => PickerAction::deserialize(action.into_deserializer())?,
        };
        Ok(Self(action))
    }
}

/// User-facing `[keys]` table keeps editor and component schemas separate.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Keys {
    normal: Option<keymap::KeyTrieNode>,
    insert: Option<keymap::KeyTrieNode>,
    select: Option<keymap::KeyTrieNode>,
    prompt: Keymap<PromptAction>,
    picker: Keymap<PickerAction>,
    buffer_picker: Keymap<BufferPickerAction>,
    file_explorer: Keymap<FileExplorerAction>,
    menu: Keymap<MenuAction>,
    popup: Keymap<PopupAction>,
    hover: Keymap<PageAction>,
    signature_help: Keymap<PageAction>,
    blame: Keymap<BlameAction>,
}

impl Keys {
    fn apply(
        self,
        editor: &mut HashMap<Mode, KeyTrie>,
        components: &mut ComponentKeymaps,
    ) -> (Keymap<PickerAction>, Keymap<PickerAction>) {
        for (mode, keys) in [
            (Mode::Normal, self.normal),
            (Mode::Insert, self.insert),
            (Mode::Select, self.select),
        ] {
            if let Some(keys) = keys {
                editor
                    .get_mut(&mode)
                    .unwrap()
                    .node_mut()
                    .unwrap()
                    .merge(keys);
            }
        }
        components.prompt.merge(self.prompt);
        components.picker.merge(self.picker);
        components.menu.merge(self.menu);
        components.popup.merge(self.popup);
        components.hover.merge(self.hover);
        components.signature_help.merge(self.signature_help);
        components.blame.merge(self.blame);
        (
            self.buffer_picker.map_actions(|action| action.0),
            self.file_explorer.map_actions(|action| action.0),
        )
    }
}

pub(super) fn resolve_keymaps(
    configured: impl IntoIterator<Item = Keys>,
) -> (HashMap<Mode, KeyTrie>, ComponentKeymaps) {
    let mut editor = keymap::default();
    let mut components = ComponentKeymaps::default();
    let mut buffer_overrides = Keymap::default();
    let mut file_explorer_overrides = Keymap::default();
    let mut file_explorer_extras = components.file_explorer.clone();
    for key in components.picker.0.keys() {
        file_explorer_extras.0.remove(key);
    }
    for keys in configured {
        let (buffer, file_explorer) = keys.apply(&mut editor, &mut components);
        buffer_overrides.merge(buffer);
        file_explorer_overrides.merge(file_explorer);
    }
    // Resolve inheritance after both configuration layers: component-specific
    // bindings take priority even over workspace changes to shared bindings.
    components.buffer_picker = components.picker.clone();
    components.buffer_picker.merge(buffer_overrides);
    // Keep file-explorer-only defaults while following every shared picker
    // binding, including keys that project configuration removed or rebound.
    file_explorer_extras
        .0
        .retain(|key, _| !components.picker.0.contains_key(key));
    components.file_explorer = components.picker.clone();
    components.file_explorer.merge(file_explorer_extras);
    components.file_explorer.merge(file_explorer_overrides);
    (editor, components)
}
