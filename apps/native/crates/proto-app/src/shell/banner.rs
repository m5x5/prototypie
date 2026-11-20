//! A banner that stays while the app runs degraded: another Prototypie holds the data dir (this
//! window shows a read-only copy), the data couldn't be opened, or the embedded API server (:3124)
//! didn't start. Unlike a notice it doesn't time out: previews and the agent queue stay broken
//! until it's fixed. It sits under the header on the left, clear of the inspector.

use rune::{Color, El, TextStyle, col, el, row, text};

use crate::app::data::Source;
use crate::app::insets::{EDGE, INSPECTOR_TOP, INSPECTOR_W};
use crate::app::server::ServerState;
use crate::app::state::AppState;
use crate::app::style::{self, Palette};

const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
const BODY: TextStyle = TextStyle::new(12.5, 400.0);

/// What the banner says, and whether it offers Retry (the server).
#[derive(Clone, Debug, PartialEq)]
pub struct Banner {
    pub title: String,
    pub body: String,
    pub retry: bool,
}

pub fn banner(s: &AppState) -> Option<Banner> {
    let b = |title: String, body: String, retry| Some(Banner { title, body, retry });
    match &s.data.source {
        Source::Locked { pid } => {
            let who = if *pid == 0 { String::new() } else { format!(" (pid {pid})") };
            return b(
                format!("Prototypie is already running{who}"),
                "This window shows a read-only copy: changes here aren't saved. Quit the other one and reopen this to edit.".into(),
                false,
            );
        }
        Source::Failed(e) => return b("Couldn't open your data".into(), format!("{e}. Nothing is saved in this window."), false),
        _ => {}
    }
    match &s.server.state {
        ServerState::Failed(e) => b(
            "Local API isn't running".into(),
            format!("{e}. Previews and the agent queue need it; another Prototypie or a dev server may hold the port."),
            true,
        ),
        _ => None,
    }
}

pub fn view(s: &AppState, p: &Palette) -> Option<El> {
    let n = banner(s)?;
    let body = col().gap(2.0).min_w(0.0).grow(1.0)
        .child(text(n.title.clone()).font(TITLE).color(p.fg))
        .child(text(n.body.clone()).font(BODY).color(p.muted_fg).wrap());
    let mut card = row().key("banner").role("alert").label(n.title.clone()).gap(12.0).px(14.0).py(10.0).radius(12.0).center_y()
        .w(INSPECTOR_W).child(body);
    if n.retry {
        card = card.child(row().key("banner-retry").role("button").label("Retry").tab_index(1).h(26.0).px(10.0).radius(8.0).center_y()
            .fill(p.fg).press(Color::BLACK.alpha(0.15))
            .on_press(|s: &mut AppState| s.retry_server())
            .child(text("Retry").font(TITLE).color(p.canvas_bg)));
    }
    Some(el().key("banner-anchor").pin(Some(EDGE), Some(INSPECTOR_TOP), None, None).child(style::glass(card, p, 0.9)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_why_the_app_is_degraded() {
        let mut s = AppState::new();
        assert_eq!(banner(&s), None);
        s.server.state = ServerState::Failed("127.0.0.1:3124: Address already in use".into());
        let b = banner(&s).unwrap();
        assert!(b.retry && b.body.contains("3124"), "{b:?}");
        // The data dir being held wins: that's the bigger problem.
        s.data.source = Source::Locked { pid: 4242 };
        let b = banner(&s).unwrap();
        assert!(!b.retry && b.title.contains("pid 4242") && b.body.contains("read-only"), "{b:?}");
    }
}
