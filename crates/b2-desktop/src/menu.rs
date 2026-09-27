//! The app's macOS menu bar, declared rather than inherited from `Menu::default()`
//! (ADR-0017, #119). AppKit dispatches menu chords before the webview sees the key, so K1
//! needs them enumerable: [`MENU`] is read by [`build`] and by [`chords`] (the UI's
//! reference sheet and conflict check).
//!
//! The native items stay predefined because they route Cut/Copy/Paste into the webview.
//! muda assigns their accelerators and exposes no getter, so the `keys` column restates
//! them; fix it here if a muda release moves one.

use serde::Serialize;
use tauri::menu::{AboutMetadata, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

/// Emitted when one of B2's own items is chosen, with the item's [`ItemSpec::id`]. Must
/// match `ui/src/api.ts`. The zoom rule itself lives in `ui/src/zoom.ts`.
pub const MENU_COMMAND_EVENT: &str = "menu-command";

/// The native behavior an item delegates to, one per [`PredefinedMenuItem`] constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    About,
    Services,
    Hide,
    HideOthers,
    Quit,
    CloseWindow,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    /// macOS's name for `maximize`.
    Zoom,
    Separator,
    /// B2's own item: emits [`MENU_COMMAND_EVENT`] with the row's id. Unlike predefined
    /// rows, its accelerator is B2's choice, derived from `keys` ([`muda_accelerator`]).
    Command,
}

/// One line of the menu.
#[derive(Debug, Clone, Copy)]
struct ItemSpec {
    /// Stable id the UI joins on (`edit.copy`). Not prefixed `menu.`, which the registry
    /// uses for the right-click menu.
    id: &'static str,
    item: Item,
    /// Shown in both the menu and the keyboard reference.
    label: &'static str,
    /// The item's chord in `ui/src/bindings.ts`'s (CodeMirror's) syntax, or `None`.
    keys: Option<&'static str>,
}

/// A section of the menu bar.
#[derive(Debug, Clone, Copy)]
struct SectionSpec {
    title: &'static str,
    items: &'static [ItemSpec],
}

const SEPARATOR: ItemSpec = ItemSpec {
    id: "separator",
    item: Item::Separator,
    label: "",
    keys: None,
};

/// B2's menu bar, in the order it is drawn.
const MENU: &[SectionSpec] = &[
    // Matches tauri.conf.json's `productName`, as `Menu::default` would pass.
    SectionSpec {
        title: "B2",
        items: &[
            ItemSpec {
                id: "app.about",
                item: Item::About,
                label: "About B2",
                keys: None,
            },
            SEPARATOR,
            ItemSpec {
                id: "app.services",
                item: Item::Services,
                label: "Services",
                keys: None,
            },
            SEPARATOR,
            ItemSpec {
                id: "app.hide",
                item: Item::Hide,
                label: "Hide B2",
                keys: Some("Mod-h"),
            },
            ItemSpec {
                id: "app.hide-others",
                item: Item::HideOthers,
                label: "Hide Others",
                keys: Some("Mod-Alt-h"),
            },
            SEPARATOR,
            ItemSpec {
                id: "app.quit",
                item: Item::Quit,
                label: "Quit B2",
                keys: Some("Mod-q"),
            },
        ],
    },
    SectionSpec {
        title: "File",
        items: &[ItemSpec {
            id: "file.close-window",
            item: Item::CloseWindow,
            label: "Close Window",
            keys: Some("Mod-w"),
        }],
    },
    // Routes the editing selectors into the webview: how copy and paste work at all.
    SectionSpec {
        title: "Edit",
        items: &[
            ItemSpec {
                id: "edit.undo",
                item: Item::Undo,
                label: "Undo",
                keys: Some("Mod-z"),
            },
            ItemSpec {
                id: "edit.redo",
                item: Item::Redo,
                label: "Redo",
                keys: Some("Mod-Shift-z"),
            },
            SEPARATOR,
            ItemSpec {
                id: "edit.cut",
                item: Item::Cut,
                label: "Cut",
                keys: Some("Mod-x"),
            },
            ItemSpec {
                id: "edit.copy",
                item: Item::Copy,
                label: "Copy",
                keys: Some("Mod-c"),
            },
            ItemSpec {
                id: "edit.paste",
                item: Item::Paste,
                label: "Paste",
                keys: Some("Mod-v"),
            },
            ItemSpec {
                id: "edit.select-all",
                item: Item::SelectAll,
                label: "Select All",
                keys: Some("Mod-a"),
            },
        ],
    },
    // Zoom lives here, not in `ui/src/bindings.ts`: a menu chord never reaches the webview,
    // and macOS expects these in the View menu.
    SectionSpec {
        title: "View",
        items: &[
            ItemSpec {
                id: "view.zoom-in",
                item: Item::Command,
                label: "Zoom In",
                keys: Some("Mod-="),
            },
            ItemSpec {
                id: "view.zoom-out",
                item: Item::Command,
                label: "Zoom Out",
                keys: Some("Mod--"),
            },
            ItemSpec {
                id: "view.zoom-reset",
                item: Item::Command,
                label: "Actual Size",
                keys: Some("Mod-0"),
            },
            SEPARATOR,
            ItemSpec {
                id: "view.fullscreen",
                item: Item::Fullscreen,
                label: "Toggle Full Screen",
                keys: Some("Mod-Ctrl-f"),
            },
        ],
    },
    SectionSpec {
        title: "Window",
        items: &[
            ItemSpec {
                id: "window.minimize",
                item: Item::Minimize,
                label: "Minimize",
                keys: Some("Mod-m"),
            },
            ItemSpec {
                id: "window.zoom",
                item: Item::Zoom,
                label: "Zoom",
                keys: None,
            },
        ],
    },
];

/// One menu item that carries a chord, sent to the UI by `menu_chords`. Borrowed because
/// [`MENU`] is static.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MenuChord {
    pub id: &'static str,
    pub label: &'static str,
    /// The chord, in `ui/src/bindings.ts`'s syntax (`Mod-Shift-z`).
    pub keys: &'static str,
}

/// Every chord the menu bar takes, in menu order. Mirrored by `ui/src/menukeys.ts`.
pub fn chords() -> Vec<MenuChord> {
    MENU.iter()
        .flat_map(|section| section.items)
        .filter_map(|spec| {
            spec.keys.map(|keys| MenuChord {
                id: spec.id,
                label: spec.label,
                keys,
            })
        })
        .collect()
}

/// A registry chord in Tauri's accelerator syntax (`Mod-Shift-z` → `CmdOrCtrl+Shift+z`).
/// Derived so a command row has one spelling: Tauri silently drops an accelerator it can't
/// parse. Splits on CodeMirror's `-(?!$)`, since `Mod--` is ⌘ plus the hyphen.
fn muda_accelerator(chord: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = chord;
    while let Some(i) = rest[..rest.len().saturating_sub(1)].find('-') {
        parts.push(&rest[..i]);
        rest = &rest[i + 1..];
    }
    let mods = parts.iter().map(|m| match *m {
        "Mod" => "CmdOrCtrl",
        other => other,
    });
    mods.chain(std::iter::once(rest))
        .collect::<Vec<_>>()
        .join("+")
}

/// Build the menu [`MENU`] describes — what `tauri::Builder::menu` installs.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = about_metadata(app);
    let menu = Menu::new(app)?;
    for section in MENU {
        let submenu = Submenu::new(app, section.title, true)?;
        for spec in section.items {
            submenu.append(item_for(app, spec, &about)?.as_ref())?;
        }
        menu.append(&submenu)?;
    }
    Ok(menu)
}

/// One [`ItemSpec`] as its native item: predefined, or B2's own for [`Item::Command`].
fn item_for<R: Runtime>(
    app: &AppHandle<R>,
    spec: &ItemSpec,
    about: &AboutMetadata<'static>,
) -> tauri::Result<Box<dyn IsMenuItem<R>>> {
    if spec.item == Item::Command {
        // The menu id is the payload the frontend switches on.
        let accel = spec.keys.map(muda_accelerator);
        let item = MenuItem::with_id(app, spec.id, spec.label, true, accel)?;
        return Ok(Box::new(item));
    }
    Ok(Box::new(predefined(app, spec, about)?))
}

/// The About panel's contents, from the same sources `Menu::default` reads. `'static`
/// because the only borrowed field, `icon`, is unset.
fn about_metadata<R: Runtime>(app: &AppHandle<R>) -> AboutMetadata<'static> {
    let pkg = app.package_info();
    let bundle = &app.config().bundle;
    AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        copyright: bundle.copyright.clone(),
        authors: bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    }
}

/// One [`ItemSpec`] as the predefined item it delegates to, with its own `label`.
fn predefined<R: Runtime>(
    app: &AppHandle<R>,
    spec: &ItemSpec,
    about: &AboutMetadata<'static>,
) -> tauri::Result<PredefinedMenuItem<R>> {
    let text = Some(spec.label);
    match spec.item {
        Item::About => PredefinedMenuItem::about(app, text, Some(about.clone())),
        Item::Services => PredefinedMenuItem::services(app, text),
        Item::Hide => PredefinedMenuItem::hide(app, text),
        Item::HideOthers => PredefinedMenuItem::hide_others(app, text),
        Item::Quit => PredefinedMenuItem::quit(app, text),
        Item::CloseWindow => PredefinedMenuItem::close_window(app, text),
        Item::Undo => PredefinedMenuItem::undo(app, text),
        Item::Redo => PredefinedMenuItem::redo(app, text),
        Item::Cut => PredefinedMenuItem::cut(app, text),
        Item::Copy => PredefinedMenuItem::copy(app, text),
        Item::Paste => PredefinedMenuItem::paste(app, text),
        Item::SelectAll => PredefinedMenuItem::select_all(app, text),
        Item::Fullscreen => PredefinedMenuItem::fullscreen(app, text),
        Item::Minimize => PredefinedMenuItem::minimize(app, text),
        Item::Zoom => PredefinedMenuItem::maximize(app, text),
        Item::Separator => PredefinedMenuItem::separator(app),
        // Unreachable (`item_for` handles it); a panic here would mean no window.
        Item::Command => PredefinedMenuItem::separator(app),
    }
}

#[cfg(test)]
mod tests {
    //! The menu as data: [`build`] needs a running app, so these check the table the UI
    //! relies on.

    use super::*;
    use std::collections::HashSet;

    /// Every item, separators included.
    fn all_items() -> impl Iterator<Item = &'static ItemSpec> {
        MENU.iter().flat_map(|section| section.items)
    }

    /// Is this spelled the way `ui/src/bindings.ts`'s `parseChord` reads a chord? A small
    /// check against Tauri's spelling (`CmdOrCtrl+C`) leaking in; `menukeys.test.ts` runs
    /// the real parser.
    fn is_registry_chord(spec: &str) -> bool {
        // The same `-(?!$)` cut as `muda_accelerator`.
        let mut parts: Vec<&str> = Vec::new();
        let mut key = spec;
        while let Some(i) = key[..key.len().saturating_sub(1)].find('-') {
            parts.push(&key[..i]);
            key = &key[i + 1..];
        }
        // One character, never uppercase: `parseChord` lowercases what it reads.
        let key_ok = key.len() == 1 && !key.chars().any(|c| c.is_ascii_uppercase());
        key_ok
            && parts
                .iter()
                .all(|m| matches!(*m, "Mod" | "Ctrl" | "Shift" | "Alt"))
    }

    #[test]
    fn every_item_has_a_unique_id_and_a_label() {
        let mut seen = HashSet::new();
        for spec in all_items() {
            if spec.item == Item::Separator {
                continue;
            }
            assert!(seen.insert(spec.id), "duplicate menu item id: {}", spec.id);
            assert!(
                !spec.label.is_empty(),
                "menu item with no label: {}",
                spec.id
            );
        }
    }

    #[test]
    fn a_separator_is_never_keyboard_surface() {
        for spec in all_items().filter(|s| s.item == Item::Separator) {
            assert!(spec.keys.is_none(), "a separator with a chord");
            assert!(spec.label.is_empty(), "a separator with a label");
        }
    }

    #[test]
    fn no_two_items_answer_to_the_same_chord() {
        // `Menu::default` binds ⌘W in both File and Window; B2 drops the duplicate.
        let mut seen = HashSet::new();
        for c in chords() {
            assert!(
                seen.insert(c.keys),
                "two menu items on {}: {}",
                c.keys,
                c.id
            );
        }
    }

    #[test]
    fn every_chord_is_spelled_the_way_the_ui_registry_reads_one() {
        for c in chords() {
            assert!(
                is_registry_chord(c.keys),
                "{} is spelled {:?}, which ui/src/bindings.ts cannot parse",
                c.id,
                c.keys
            );
        }
        assert!(!is_registry_chord("CmdOrCtrl+C"));
        assert!(!is_registry_chord("Mod-Meh-c"));
        assert!(!is_registry_chord("Mod-C"));
        assert!(is_registry_chord("Mod--"));
        assert!(is_registry_chord("Mod-="));
    }

    #[test]
    fn a_command_item_carries_a_chord_muda_can_actually_parse() {
        // Tauri drops an unparseable accelerator silently, so pin what this emits.
        assert_eq!(muda_accelerator("Mod-="), "CmdOrCtrl+=");
        assert_eq!(muda_accelerator("Mod--"), "CmdOrCtrl+-");
        assert_eq!(muda_accelerator("Mod-0"), "CmdOrCtrl+0");
        assert_eq!(muda_accelerator("Mod-Shift-z"), "CmdOrCtrl+Shift+z");
        assert_eq!(muda_accelerator("Mod-Ctrl-f"), "CmdOrCtrl+Ctrl+f");
        assert_eq!(muda_accelerator("-"), "-");
        assert_eq!(muda_accelerator("f"), "f");

        for spec in all_items().filter(|s| s.item == Item::Command) {
            let keys = spec.keys.unwrap_or_else(|| panic!("{}: no chord", spec.id));
            let accel = muda_accelerator(keys);
            let mut tokens = accel.split('+').collect::<Vec<_>>();
            let key = tokens.pop().unwrap_or_default();
            assert_eq!(key.chars().count(), 1, "{}: key is {key:?}", spec.id);
            for t in tokens {
                assert!(
                    matches!(t, "CmdOrCtrl" | "Ctrl" | "Shift" | "Alt"),
                    "{}: muda doesn't know the modifier {t:?}",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn a_command_item_is_addressable_and_every_other_item_is_not() {
        for spec in all_items().filter(|s| s.item == Item::Command) {
            assert!(!spec.id.is_empty(), "a command item with no id");
            assert!(
                spec.keys.is_some(),
                "{}: a command item with no chord — it would be mouse-only (K1)",
                spec.id
            );
        }
    }

    #[test]
    fn the_exported_chords_are_what_the_ui_mirrors() {
        // `ui/src/menukeys.ts` carries this same list; change the two together (the app
        // also reports drift at startup, `menuDrift`).
        let exported: Vec<String> = chords()
            .iter()
            .map(|c| format!("{} {} {}", c.id, c.keys, c.label))
            .collect();
        assert_eq!(
            exported,
            [
                "app.hide Mod-h Hide B2",
                "app.hide-others Mod-Alt-h Hide Others",
                "app.quit Mod-q Quit B2",
                "file.close-window Mod-w Close Window",
                "edit.undo Mod-z Undo",
                "edit.redo Mod-Shift-z Redo",
                "edit.cut Mod-x Cut",
                "edit.copy Mod-c Copy",
                "edit.paste Mod-v Paste",
                "edit.select-all Mod-a Select All",
                "view.zoom-in Mod-= Zoom In",
                "view.zoom-out Mod-- Zoom Out",
                "view.zoom-reset Mod-0 Actual Size",
                "view.fullscreen Mod-Ctrl-f Toggle Full Screen",
                "window.minimize Mod-m Minimize",
            ]
        );
    }
}
