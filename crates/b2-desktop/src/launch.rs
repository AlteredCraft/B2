//! What the window looks like before the app does, and how long that lasts (GH #225).
//!
//! Tauri shows a window as soon as it exists, which is well before the webview has a
//! stylesheet, let alone a shell — so the first thing a launch shows is the platform's
//! white. Two halves of an answer live here, and neither hides the window: the reverted
//! attempt at that (reveal-on-signal from the page) died on WebKit throttling
//! `requestAnimationFrame` in an offscreen window, and traded a flash for a fixed
//! multi-second wait.
//!
//! **The ground.** [`ground`] maps the OS theme to B2's `--bg`, and `main` hands it to
//! the window before the event loop runs. This covers the instant no frontend change can
//! reach — before any HTML exists — and it is the *only* half that can, which is also why
//! it is the weaker half: the theme a user pinned in Settings lives in the webview's
//! `localStorage`, deliberately (a viewing choice is never host state), so the host can
//! only ask the OS. Right for the "System" default, and one ground change for a pin that
//! disagrees with the OS.
//!
//! `ui/index.html` carries the same two values, and the pair is not one rule written
//! twice: this one dresses the **window**, and is painted over the moment the webview
//! draws an unstyled document white on top of it; that one is the **document's** own
//! ground, which is what survives from then on. Two surfaces, one colour, and
//! `ground_matches_the_stylesheet` below is what stops them drifting apart.
//!
//! **The measurement.** The issue asks for before-and-after numbers, and a launch spans
//! two clocks: the host's and the webview's. [`mark`] and [`webview_mark`] put both on
//! one axis — Unix epoch milliseconds — in the one JSONL dataset `B2_LOG_FILE` already
//! collects, under the `b2::launch` target the implied `b2=debug` filter picks up.
//! `epoch_ms` is **when the milestone happened**, on every mark and whichever side timed
//! it; a webview mark carries the host's receipt separately as `received_epoch_ms`, so
//! the IPC is a quantity you can read rather than one folded into the gap. Read a launch
//! with:
//!
//! ```text
//! B2_LOG_FILE=$PWD/logs/launch.jsonl B2_VAULT_PATH=~/notes make app
//! jq -r 'select(.target == "b2::launch") | [.mark, .epoch_ms] | @tsv' logs/launch.jsonl
//! ```
//!
//! The five marks bracket the whole gap: `window-ready` (the host has the window and its
//! ground), `page-load-started` / `page-load-finished` (the document), then the webview's
//! own `boot-start` (the module graph finished evaluating — under `tauri dev` this is the
//! half the CSS-through-JS serving widens) and `first-frame` (the app is on screen).
//!
//! Emitting is free when no subscriber is installed, which is every launch that did not
//! ask for one, so none of this is gated behind a flag the frontend would have to read.

use std::time::{SystemTime, UNIX_EPOCH};
use tauri::webview::Color;
use tauri::Theme;

/// B2's light ground — `--bg` in `ui/style.css`'s `:root`, and the same value
/// `ui/index.html` paints before the stylesheet lands (`ui/src/ground.test.ts` is
/// what keeps those two honest; this one is a third copy in a different language,
/// checked by `ground_matches_the_stylesheet` below).
const GROUND_LIGHT: Color = Color(0xfa, 0xf9, 0xf7, 0xff);

/// B2's dark ground — `--bg` under `:root[data-theme="dark"]` and the
/// `prefers-color-scheme: dark` block, which `ui/style.css` requires to be identical.
const GROUND_DARK: Color = Color(0x16, 0x16, 0x1a, 0xff);

/// The window's ground for the OS theme in force, opaque.
///
/// `None` is a platform that would not answer (`WebviewWindow::theme` is fallible, and
/// `Theme` is `#[non_exhaustive]` — a variant added upstream is a theme this build has no
/// colour for). Both fall back to light, which is what `ui/style.css`'s unqualified
/// `:root` does with the same question: the fallback is the light palette, not white.
pub fn ground(theme: Option<Theme>) -> Color {
    match theme {
        Some(Theme::Dark) => GROUND_DARK,
        _ => GROUND_LIGHT,
    }
}

/// Record a host-side launch milestone on the shared epoch axis.
pub fn mark(name: &str) {
    tracing::debug!(target: "b2::launch", mark = name, epoch_ms = epoch_ms(), "launch");
}

/// Record a milestone the *webview* timed, at the moment it says it happened.
///
/// `epoch_ms` is the webview's own reading (`performance.timeOrigin + performance.now()`),
/// **not** the host's clock at receipt, so the field means one thing on all five marks and
/// the query above is honest about every one of them. Put the receipt in `epoch_ms` and
/// the two marks that matter most would each carry an IPC hop they did not spend — the
/// instrument reporting its own latency as the app's, which is the one error a timing
/// probe must not make.
///
/// The receipt is kept as `received_epoch_ms` rather than dropped: it is the only evidence
/// that the hop was ordinary, and a launch where it is not is a launch to distrust.
pub fn webview_mark(name: &str, webview_epoch_ms: f64) {
    tracing::debug!(
        target: "b2::launch",
        mark = name,
        epoch_ms = webview_epoch_ms,
        received_epoch_ms = epoch_ms(),
        "launch"
    );
}

/// Wall-clock milliseconds since the Unix epoch — the one axis the host and the webview
/// can both name. A clock set before 1970 reports 0 rather than panicking; it would make
/// the record useless, which is the honest outcome, and never a crashed launch.
fn epoch_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_is_dark_only_for_a_dark_os() {
        assert_eq!(ground(Some(Theme::Dark)), GROUND_DARK);
        assert_eq!(ground(Some(Theme::Light)), GROUND_LIGHT);
        // The two cases that are not a theme at all: a platform that won't say, and a
        // variant this build predates. Neither may leave the window white.
        assert_eq!(ground(None), GROUND_LIGHT);
    }

    #[test]
    fn ground_matches_the_stylesheet() {
        // The third copy of these two values, and the only one in Rust. `ui/style.css` is
        // the source; `ui/index.html` holds the second copy and `ui/src/ground.test.ts`
        // pins it. Read the sheet rather than restating it, so the pair can only drift by
        // someone editing this assertion on purpose.
        let css = include_str!("../../../ui/style.css");
        let hex = |c: Color| format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2);
        let grounds: Vec<&str> = css
            .match_indices("--bg:")
            .filter_map(|(i, _)| css[i..].split(';').next())
            .map(|d| d.trim_start_matches("--bg:").trim())
            .collect();
        assert_eq!(
            grounds.first().copied(),
            Some(hex(GROUND_LIGHT).as_str()),
            "the light ground has drifted from ui/style.css's `:root`"
        );
        assert!(
            grounds.iter().skip(1).all(|g| *g == hex(GROUND_DARK)),
            "the dark ground has drifted from ui/style.css: {grounds:?}"
        );
        assert_eq!(
            grounds.len(),
            3,
            "ui/style.css no longer defines --bg three times"
        );
    }

    #[test]
    fn epoch_ms_is_a_plausible_wall_clock() {
        // 2020-01-01 in ms. Guards the units — a seconds-vs-millis slip here would make
        // every launch record silently unjoinable with the webview's.
        assert!(epoch_ms() > 1_577_836_800_000.0);
    }
}
