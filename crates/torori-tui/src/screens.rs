//! Screen composition: header (screen tabs + mode) over the
//! active screen's pure panel. Rendering only — all behaviour
//! lives in [`crate::app`].

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::Paragraph;

use crate::app::{App, Screen};
use crate::panels;

/// Draw the current screen into `frame`.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(frame.area());
    let header = Paragraph::new(format!(
        "torori-tui | screen: {:?} | mode: {:?} | 1 overview  2 workflows  3 log  4 fleet  m mode  q quit",
        app.screen, app.mode
    ));
    frame.render_widget(header, chunks[0]);
    let panel = match app.screen {
        Screen::Overview => panels::overview_panel(
            app.status.as_ref(),
            app.voice_lane.as_deref(),
            app.tool_count,
            app.engine.rules().len(),
        ),
        Screen::Workflows => panels::workflows_panel(
            app.mode,
            app.staged.as_ref(),
            &app.last_rows,
            app.last_skipped,
        ),
        Screen::Log => panels::log_panel(&app.log),
        Screen::Fleet => panels::fleet_panel(app.engine.rules(), app.engine.audit()),
    };
    frame.render_widget(panel, chunks[1]);
}
