//! Colour roles from `.docs/design-system.md`. Widgets only ever use a role; a theme fills
//! the roles in.

use ratatui::style::Color;

pub struct Theme {
    pub fg: Color,
    pub dim: Color,
    /// `None` keeps the terminal's own background.
    pub bg: Option<Color>,
    /// Text on a solid `fg` bar.
    pub on_bar: Color,
    /// Box fills and text selection.
    pub panel: Color,
    /// Cursor marker and list dots. Phosphor has no separate accent: the bar marks the cursor.
    pub accent: Color,
    pub status: Color,
    /// The collapse marker on list titles, distinct from the one on todos (`accent`).
    pub list_marker: Color,
    /// Whether the cursor row is drawn as a full-width solid bar.
    pub cursor_bar: bool,
}

/// The terminal's own colours, so the panel follows the user's terminal theme.
pub const PLAIN: Theme = Theme {
    fg: Color::Reset,
    dim: Color::DarkGray,
    bg: None,
    on_bar: Color::Black,
    panel: Color::Blue,
    accent: Color::Cyan,
    status: Color::Yellow,
    list_marker: Color::Magenta,
    cursor_bar: false,
};

/// Green phosphor: greens sampled from a Fallout 4 Pip-Boy screenshot, on a black screen.
pub const PHOSPHOR: Theme = Theme {
    fg: Color::Rgb(105, 255, 125),
    dim: Color::Rgb(62, 112, 60),
    bg: Some(Color::Rgb(0, 0, 0)),
    on_bar: Color::Rgb(0, 0, 0),
    panel: Color::Rgb(18, 48, 22),
    accent: Color::Rgb(105, 255, 125),
    status: Color::Rgb(62, 112, 60),
    // Amber, the Pip-Boy's other screen colour: stands apart from the greens.
    list_marker: Color::Rgb(255, 182, 66),
    cursor_bar: true,
};
