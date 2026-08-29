//! The app's macOS menu bar — **declared**, not inherited (ADR-0017, #119).
//!
//! Tauri applies `Menu::default()` to an app that sets none, and a dozen chords ride in
//! with it. Those chords are live in the window and used to be invisible twice over:
//! nothing enumerates the default, and AppKit dispatches a menu key equivalent inside
//! `NSApplication.sendEvent` *before* the key window's responder chain, so they never reach
//! the webview's keydown handler either. Invariant K1 promises a keyboard path that is
//! *findable*, and you cannot document what you cannot enumerate.
//!
//! So the menu is B2's own data now. [`MENU`] is the whole of it, with exactly two readers:
//! [`build`], which is what the window gets, and [`chords`], which the `menu_chords`
//! command hands the UI for the reference sheet and the registry's conflict check.
//!
//! **The items stay predefined on purpose.** The Edit menu is load-bearing rather than
//! decorative — Cut/Copy/Paste work in the webview *because* the native items route the
//! standard selectors to it. The consequence is that B2 does not *choose* these
//! accelerators: muda assigns them and exposes no getter, so the `keys` column below
//! restates them. This is the one place to fix if a muda release moves one.
//!
//! **Two departures from `Menu::default()`**, neither touching a chord: its Window menu
//! repeats Close Window (⌘W), which already lives in File, and its Help menu is empty on
//! macOS. Neither survives here.

use serde::Serialize;
use tauri::menu::{AboutMetadata, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

/// The event the host emits when one of B2's **own** menu items is chosen — the View
/// menu's three zoom lines, today. The payload is the item's [`ItemSpec::id`], and
/// `ui/src/api.ts` carries the mirror of this string; change the two together.
///
/// Why an event at all, rather than the host simply zooming. The *rule* — the ladder of
/// sizes, its walls, and remembering the choice — lives in `ui/src/zoom.ts`, because a
/// reading size is a viewing preference and this crate holds no logic (the one rule).
/// AppKit just happens to be where the keystroke lands, so the host's whole job is to
/// say which line was chosen and let the frontend decide what that means.
pub const MENU_COMMAND_EVENT: &str = "menu-command";

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
    /// **B2's own**, rather than a native behavior delegated to. The only kind of item
    /// here that has no `PredefinedMenuItem` behind it: choosing it emits
    /// [`MENU_COMMAND_EVENT`] carrying the row's id, and the frontend decides what it
    /// means. Its accelerator is derived from the row's `keys` ([`muda_accelerator`]),
    /// so — unlike every predefined row above, whose chord muda assigns and this table
    /// merely restates — this is a chord B2 actually chooses.
    Command,
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
    // The one section with items of B2's own. The three sizes are here rather than in
    // `ui/src/bindings.ts` for the reason this module exists at all: a menu accelerator
    // is dispatched before the key window's responder chain, so a chord spelled in both
    // places is a chord the webview never receives. Since macOS expects Zoom In / Zoom
    // Out / Actual Size to *be* in the View menu — with their chords printed beside them,
    // which is where most people find them — the menu is the honest owner, and the
    // registry stays out of these three keystrokes entirely.
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

/// One chord, translated from the registry's spelling into the one Tauri's accelerator
/// parser reads (`Mod-Shift-z` → `CmdOrCtrl+Shift+z`).
///
/// **Derived rather than written down**, and that is the whole point of the function: an
/// [`Item::Command`] row would otherwise carry the same chord twice — once for the UI to
/// mirror and once for muda to bind — with nothing but care keeping them equal. Tauri
/// takes the accelerator as a string and *silently drops one it can't parse*
/// (`.parse().ok()`), so the failure mode of a drifted second spelling is not an error
/// but a menu item that quietly has no shortcut. One source, no drift, no silence.
///
/// The split is CodeMirror's own rule, `-(?!$)`, for the reason `parseChord` gives: `-`
/// is both the separator and a key you can press, so `Mod--` is ⌘ plus the hyphen.
fn muda_accelerator(chord: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = chord;
    // Cut at every `-` that isn't the last character; what's left when none remains is
    // the key. `split` can't express "not at the end", so this walks it.
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

/// One [`ItemSpec`] as the native item it becomes — predefined for everything the
/// platform already does, and B2's own for [`Item::Command`].
///
/// Boxed because those are two unrelated types and a submenu takes `&dyn IsMenuItem`;
/// it is one allocation per row, once, at launch.
fn item_for<R: Runtime>(
    app: &AppHandle<R>,
    spec: &ItemSpec,
    about: &AboutMetadata<'static>,
) -> tauri::Result<Box<dyn IsMenuItem<R>>> {
    if spec.item == Item::Command {
        // `spec.id` is the payload the frontend switches on, so the item's menu id and
        // the row's id are the same string by construction.
        let accel = spec.keys.map(muda_accelerator);
        let item = MenuItem::with_id(app, spec.id, spec.label, true, accel)?;
        return Ok(Box::new(item));
    }
    Ok(Box::new(predefined(app, spec, about)?))
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
        // Unreachable: `item_for` takes this branch before calling here. Handled rather
        // than `unreachable!()` — a panic in the menu builder is a window that never
        // opens, and a separator is the harmless thing to draw if the two ever disagree.
        Item::Command => PredefinedMenuItem::separator(app),
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
        // The same `-(?!$)` cut `muda_accelerator` makes, and for the same reason: `-` is
        // both the separator and a key, so ⌘- is spelled `Mod--`.
        let mut parts: Vec<&str> = Vec::new();
        let mut key = spec;
        while let Some(i) = key[..key.len().saturating_sub(1)].find('-') {
            parts.push(&key[..i]);
            key = &key[i + 1..];
        }
        // One character, and never an uppercase one — `parseChord` lowercases what it
        // reads, so an uppercase key here is a chord that parses to something else.
        // Symbols are allowed: `=` and `-` are the View menu's.
        let key_ok = key.len() == 1 && !key.chars().any(|c| c.is_ascii_uppercase());
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
        assert!(!is_registry_chord("Mod-C"));
        // ...and it accepts the two symbol keys the View menu is spelled with.
        assert!(is_registry_chord("Mod--"));
        assert!(is_registry_chord("Mod-="));
    }

    #[test]
    fn a_command_item_carries_a_chord_muda_can_actually_parse() {
        // The one place B2 *chooses* an accelerator rather than restating one macOS
        // assigned — and Tauri drops an unparseable accelerator silently
        // (`.parse().ok()`), so a wrong spelling here is a menu line with no shortcut and
        // no complaint. `muda_accelerator` is the single source; this pins what it emits.
        assert_eq!(muda_accelerator("Mod-="), "CmdOrCtrl+=");
        assert_eq!(muda_accelerator("Mod--"), "CmdOrCtrl+-");
        assert_eq!(muda_accelerator("Mod-0"), "CmdOrCtrl+0");
        assert_eq!(muda_accelerator("Mod-Shift-z"), "CmdOrCtrl+Shift+z");
        assert_eq!(muda_accelerator("Mod-Ctrl-f"), "CmdOrCtrl+Ctrl+f");
        // A bare key keeps its lone self rather than becoming an empty modifier.
        assert_eq!(muda_accelerator("-"), "-");
        assert_eq!(muda_accelerator("f"), "f");

        // And every command row in the real table survives the trip: modifiers muda
        // knows, one key left over, nothing empty.
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
        // The event payload is the row's id, so a command row without one is a menu line
        // the frontend cannot act on. The converse matters just as much: a predefined row
        // must stay predefined, because those are what route the platform's editing
        // selectors into the webview (copy and paste work *because* of them).
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
        // `ui/src/menukeys.ts` carries this same list — the UI's only offline knowledge of
        // what the menu takes, and what its conflict gate reads. **Change the two
        // together**: the app compares them at startup (`menuDrift`) and reports a
        // mismatch, the same posture as `WRITE_CONFLICT_MESSAGE`. A row moving in or out is
        // a change to what the app reserves from its own keyboard, which is exactly what
        // used to happen invisibly — hence a pin rather than a count.
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
