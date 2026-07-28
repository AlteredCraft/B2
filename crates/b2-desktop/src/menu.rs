//! The app's macOS menu bar — **declared**, not inherited
//! ([#119](https://github.com/AlteredCraft/B2/issues/119)).
//!
//! Tauri applies `Menu::default()` to an app that sets none, and a dozen chords ride
//! in with it: ⌘Q, ⌘W, ⌘M, ⌘H, ⌥⌘H, ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V, ⌘A, ⌃⌘F. Those chords are
//! live in the window, and they used to be invisible twice over. Nothing enumerates
//! the default, so no documentation could list them; and AppKit dispatches a menu key
//! equivalent inside `NSApplication.sendEvent` *before* the key window's responder
//! chain, so they never reach the webview's keydown handler and the keyboard registry
//! (`ui/src/bindings.ts`) could not see them either. Invariant **K1**
//! (docs/design/invariants.md) promises a keyboard path that is *findable*; an
//! inherited chord sits outside that promise entirely — you cannot document what you
//! cannot enumerate.
//!
//! So the menu is B2's own data now. [`MENU`] is the whole of it — the sections, their
//! items, and the chord macOS gives each one — and it has exactly two readers:
//! [`build`], which is what the window actually gets, and [`chords`], which the
//! `menu_chords` command hands the UI so the reference sheet can list them and the
//! registry's collision check can see them (`ui/src/menukeys.ts`).
//!
//! **The items stay predefined on purpose.** The Edit menu is load-bearing rather than
//! decorative — Cut/Copy/Paste/Select All work in the webview *because* the native
//! items route the standard selectors to it, so these remain [`PredefinedMenuItem`]s
//! rather than custom items B2 would then have to implement itself. The consequence is
//! that B2 does not *choose* these accelerators: muda assigns them and exposes no
//! getter for them, so the `keys` column below restates them. This is the one place to
//! fix if a muda release ever moves one.
//!
//! **Two departures from `Menu::default()`**, neither of which touches a chord: its
//! Window menu repeats Close Window (⌘W), which already lives in File, and its Help
//! menu is empty on macOS. Neither survives here.

use serde::Serialize;
use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

/// The native behavior an item delegates to — one variant per [`PredefinedMenuItem`]
/// constructor B2 uses. An enum rather than a function pointer so [`MENU`] stays a
/// plain, readable table that the tests below can walk.
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
    /// macOS's name for `maximize` — the label the platform itself uses.
    Zoom,
    Separator,
}

/// One line of the menu.
#[derive(Debug, Clone, Copy)]
struct ItemSpec {
    /// Stable id, and the join key the UI mirrors this row by (`edit.copy`).
    /// Deliberately *not* prefixed `menu.`: the registry already spells the
    /// right-click menu's own commands that way (`menu.open`, `menu.item.next`).
    id: &'static str,
    item: Item,
    /// What the menu shows — and, since the reference sheet renders these verbatim,
    /// what the keyboard reference calls the action. One string, both places.
    label: &'static str,
    /// The chord macOS gives this item, spelled in the chord syntax of
    /// `ui/src/bindings.ts` (which is CodeMirror's) so the UI can parse it with the
    /// same parser it uses for B2's own chords. `None` for an item with no
    /// accelerator — those are real menu items, but they are not keyboard surface.
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
    // The application menu. macOS draws this one from the bundle, and `Menu::default`
    // passes the package name here — which is this same string (tauri.conf.json's
    // `productName`).
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
    // The load-bearing one: these route the platform's editing selectors into the
    // webview, which is how copy and paste work at all inside the note editor.
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
    SectionSpec {
        title: "View",
        items: &[ItemSpec {
            id: "view.fullscreen",
            item: Item::Fullscreen,
            label: "Toggle Full Screen",
            keys: Some("Mod-Ctrl-f"),
        }],
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

/// One menu item that carries a chord — the host's half of the app's keyboard
/// contract, serialized to the UI by the `menu_chords` command.
///
/// Borrowed rather than owned because [`MENU`] is static: there is nothing to build,
/// only something to hand over.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MenuChord {
    pub id: &'static str,
    pub label: &'static str,
    /// The chord, in `ui/src/bindings.ts`'s syntax (`Mod-Shift-z`).
    pub keys: &'static str,
}

/// Every chord the menu bar takes, in menu order.
///
/// Items with no accelerator are skipped: this is the keyboard surface, not an
/// inventory of the menu. `ui/src/menukeys.ts` mirrors the result — see the pin in
/// this module's tests.
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

/// Build the menu [`MENU`] describes — what `tauri::Builder::menu` installs.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = about_metadata(app);
    let menu = Menu::new(app)?;
    for section in MENU {
        let submenu = Submenu::new(app, section.title, true)?;
        for spec in section.items {
            submenu.append(&predefined(app, spec, &about)?)?;
        }
        menu.append(&submenu)?;
    }
    Ok(menu)
}

/// The About panel's contents, from the same sources `Menu::default` reads: the
/// package info and the bundle config.
///
/// `'static` because the only borrowed field is the panel's `icon`, which B2 leaves
/// unset — eliding it here would tie the metadata to the handle it was read from for
/// no reason.
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

/// One [`ItemSpec`] as the native item it delegates to. Every item passes its own
/// `label`, so what the menu shows and what the keyboard reference prints are the
/// same string rather than two that agree today.
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
    }
}

#[cfg(test)]
mod tests {
    //! The menu as *data*. [`build`] needs a running app and is left to the app to
    //! exercise; the table it reads is what has to hold together, and every check here
    //! is a claim the UI relies on — a chord it can parse, an id it can join by, and a
    //! list that matches its mirror.

    use super::*;
    use std::collections::HashSet;

    /// Every item, separators included.
    fn all_items() -> impl Iterator<Item = &'static ItemSpec> {
        MENU.iter().flat_map(|section| section.items)
    }

    /// Is this spelled the way `ui/src/bindings.ts`'s `parseChord` reads a chord?
    ///
    /// A deliberately small check, not a second parser: it exists to catch a chord
    /// written in the *platform's* spelling (`CmdOrCtrl+C`, which is what Tauri's own
    /// accelerator syntax would want) leaking into a table the UI parses with
    /// CodeMirror's. `menukeys.test.ts` runs the real parser over the mirror.
    fn is_registry_chord(spec: &str) -> bool {
        let mut parts = spec.split('-').collect::<Vec<_>>();
        let Some(key) = parts.pop() else {
            return false;
        };
        let key_ok = key.len() == 1
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        key_ok
            && parts
                .iter()
                .all(|m| matches!(*m, "Mod" | "Ctrl" | "Shift" | "Alt"))
    }

    #[test]
    fn every_item_has_a_unique_id_and_a_label() {
        // The id is what the UI's mirror joins on, so a duplicate would make one of the
        // two rows unaddressable; the label is what both the menu and the keyboard
        // reference print, so an empty one is a blank row in the sheet.
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
        // The sheet lists one action per chord, so a menu that binds ⌘W twice — which
        // `Menu::default` does, with Close Window in both File and Window — would print
        // two rows the reader can't choose between. Dropping that duplicate is one of
        // this module's two departures from the default.
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
        // And the guard has teeth: the spelling this is here to keep out.
        assert!(!is_registry_chord("CmdOrCtrl+C"));
        assert!(!is_registry_chord("Mod-Meh-c"));
    }

    #[test]
    fn the_exported_chords_are_what_the_ui_mirrors() {
        // `ui/src/menukeys.ts` carries this same list — it is the UI's only offline
        // knowledge of what the menu takes, and what its collision gate reads. **Change
        // the two together**: the app compares them at startup (`menuDrift`, called from
        // the frontend's boot) and reports a mismatch, the same posture as
        // `WRITE_CONFLICT_MESSAGE` and `VAULT_CHANGED_EVENT`.
        //
        // A row moving in or out of this list is a change to what the app reserves from
        // its own keyboard, which is exactly the kind of thing that used to happen
        // invisibly — hence a pin rather than a count.
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
                "view.fullscreen Mod-Ctrl-f Toggle Full Screen",
                "window.minimize Mod-m Minimize",
            ]
        );
    }
}
