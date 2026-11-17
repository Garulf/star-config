use iced::border::{self, Border};
use iced::font::Weight;
use iced::widget::{
    button, checkbox, container, overlay::menu, pick_list, progress_bar, rule, scrollable, slider,
    svg, text, text_editor, text_input, toggler,
};
use iced::{Background, Color, Font, Shadow, Theme, Vector};
use sc_core::settings::ThemeChoice;

pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub mod size {
    pub const CAPTION: f32 = 12.0;
    pub const BODY: f32 = 14.0;
    pub const LABEL: f32 = 15.0;
    pub const TITLE: f32 = 20.0;
    pub const DISPLAY: f32 = 26.0;
}

pub mod radius {
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const PILL: f32 = 999.0;
}

pub const SIDEBAR_WIDTH: f32 = 248.0;
pub const TOPBAR_HEIGHT: f32 = 56.0;

/// Fixed dimensions of kit widgets. Screens pass these instead of literals.
pub mod layout {
    pub const BOTTOMBAR_HEIGHT: f32 = 56.0;
    pub const CONTENT_MAX_WIDTH: f32 = 960.0;
    pub const CONTROL_WIDTH: f32 = 240.0;
    /// The slot right of a control that holds a row action such as the remove icon.
    pub const ROW_ACTION_WIDTH: f32 = 32.0;
    /// A dense list row that holds a text field.
    pub const EDIT_ROW_HEIGHT: f32 = 48.0;
    /// A line of the Game settings list: a label, a name and up to two hint lines.
    pub const GAME_ROW_HEIGHT: f32 = 64.0;
    pub const NUMBER_WIDTH: f32 = 96.0;
    pub const VALUE_WIDTH: f32 = 40.0;
    pub const WIDE_CONTROL_WIDTH: f32 = 360.0;
    pub const WINDOW_WIDTH: f32 = 1280.0;
    pub const WINDOW_HEIGHT: f32 = 800.0;
    pub const WINDOW_MIN_WIDTH: f32 = 1000.0;
    pub const WINDOW_MIN_HEIGHT: f32 = 640.0;
    pub const NOTICE_WIDTH: f32 = 420.0;
    pub const NOTICE_HEIGHT: f32 = 160.0;
    pub const SEARCH_WIDTH: f32 = 260.0;
    pub const INSTALL_PICK_WIDTH: f32 = 320.0;
    pub const DIALOG_WIDTH: f32 = 460.0;
    pub const TOAST_WIDTH: f32 = 400.0;
    pub const ROW_MIN_HEIGHT: f32 = 52.0;
    pub const NAV_HEIGHT: f32 = 36.0;
    pub const TABLE_ROW_HEIGHT: f32 = 36.0;
    pub const CONTROL_HEIGHT: f32 = 34.0;
    pub const TOGGLER_SIZE: f32 = 20.0;
    pub const CHECKBOX_SIZE: f32 = 18.0;
    pub const MARKER_WIDTH: f32 = 4.0;
    pub const MARKER_HEIGHT: f32 = 20.0;
    pub const DOT: f32 = 8.0;
    pub const PROGRESS_HEIGHT: f32 = 6.0;
    pub const ICON: f32 = 16.0;
    pub const ICON_EMPTY: f32 = 28.0;
    pub const ICON_EMPTY_FRAME: f32 = 56.0;
    pub const SPLIT_LEFT: f32 = 320.0;
    pub const SLIDER_WIDTH: f32 = 200.0;
    pub const VIRTUAL_FALLBACK_HEIGHT: f32 = 2400.0;
    pub const DIFF_ROW_HEIGHT: f32 = 22.0;
    pub const DIFF_SIGN_WIDTH: f32 = 16.0;
    pub const SHOT_TILE_MIN_WIDTH: f32 = 220.0;
    pub const SHOT_TILE_ASPECT: f32 = 9.0 / 16.0;
    pub const SHOT_TILE_RING: f32 = 2.0;
    pub const SHOT_HEADING_HEIGHT: f32 = 44.0;
    pub const SHOT_PERIOD_WIDTH: f32 = 150.0;
    pub const SHOT_INFO_WIDTH: f32 = 320.0;
    pub const SHOT_DETAIL_LABEL_WIDTH: f32 = 84.0;
    /// Overview cards: three across at the default window, two at the minimum.
    pub const OVERVIEW_CARD_MAX_WIDTH: f32 = 400.0;
    pub const OVERVIEW_CARD_HEIGHT: f32 = 200.0;
    pub const OVERVIEW_THUMB_HEIGHT: f32 = 32.0;
}

const BORDER_WIDTH: f32 = 1.0;
const TOGGLER_PADDING_RATIO: f32 = 0.18;
const SLIDER_RAIL_WIDTH: f32 = 4.0;
const SLIDER_HANDLE_RADIUS: f32 = 7.0;
const SCROLLER_WIDTH: f32 = 3.0;

pub const FONT: Font = Font::with_name("Fira Sans");
pub const FONT_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..FONT
};
pub const FONT_MONO: Font = Font::MONOSPACE;
pub const FONT_SEMIBOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/FiraSans-SemiBold.ttf");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub surface_raised: Color,
    pub border: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

const DARK: Palette = Palette {
    bg: Color::from_rgb8(0x0E, 0x11, 0x16),
    surface: Color::from_rgb8(0x14, 0x18, 0x1F),
    surface_raised: Color::from_rgb8(0x1B, 0x20, 0x29),
    border: Color::from_rgb8(0x2A, 0x30, 0x3B),
    text: Color::from_rgb8(0xE6, 0xE9, 0xEF),
    text_muted: Color::from_rgb8(0x98, 0xA2, 0xB3),
    accent: Color::from_rgb8(0xF2, 0xA7, 0x3B),
    accent_text: Color::from_rgb8(0x1C, 0x12, 0x03),
    success: Color::from_rgb8(0x55, 0xC0, 0x8C),
    warning: Color::from_rgb8(0xE8, 0xC4, 0x5C),
    danger: Color::from_rgb8(0xE5, 0x5A, 0x50),
};

const LIGHT: Palette = Palette {
    bg: Color::from_rgb8(0xE8, 0xEB, 0xEF),
    surface: Color::from_rgb8(0xF4, 0xF5, 0xF7),
    surface_raised: Color::from_rgb8(0xFF, 0xFF, 0xFF),
    border: Color::from_rgb8(0xD3, 0xD8, 0xDF),
    text: Color::from_rgb8(0x16, 0x1A, 0x21),
    text_muted: Color::from_rgb8(0x55, 0x5E, 0x6E),
    accent: Color::from_rgb8(0xEE, 0x9A, 0x22),
    accent_text: Color::from_rgb8(0x1C, 0x12, 0x03),
    success: Color::from_rgb8(0x1E, 0x8A, 0x5A),
    warning: Color::from_rgb8(0xA8, 0x78, 0x10),
    danger: Color::from_rgb8(0xC9, 0x3A, 0x32),
};

pub fn palette(choice: ThemeChoice) -> Palette {
    match choice {
        ThemeChoice::Dark => DARK,
        ThemeChoice::Light => LIGHT,
    }
}

pub fn theme(choice: ThemeChoice) -> Theme {
    let p = palette(choice);
    let name = match choice {
        ThemeChoice::Dark => "star-config dark",
        ThemeChoice::Light => "star-config light",
    };
    Theme::custom(
        name,
        iced::theme::Palette {
            background: p.surface,
            text: p.text,
            primary: p.accent,
            success: p.success,
            warning: p.warning,
            danger: p.danger,
        },
    )
}

/// The token palette behind any theme built by [`theme`].
pub fn tokens(theme: &Theme) -> Palette {
    if theme.extended_palette().is_dark {
        DARK
    } else {
        LIGHT
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn hover_of(base: Color, p: &Palette) -> Color {
    mix(base, p.text, 0.06)
}

fn pressed_of(base: Color, p: &Palette) -> Color {
    mix(base, p.bg, 0.25)
}

fn outline(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: BORDER_WIDTH,
        radius: radius.into(),
    }
}

/// Semantic colour role shared by tags, icons, dots and banners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Tone::Neutral => p.text,
            Tone::Muted => p.text_muted,
            Tone::Accent => p.accent,
            Tone::Success => p.success,
            Tone::Warning => p.warning,
            Tone::Danger => p.danger,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
    DangerGhost,
}

// Containers

pub fn card(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface_raised.into()),
        border: outline(p.border, radius::MD),
        ..container::Style::default()
    }
}

/// A card that needs the user's attention: the card with a warning-tinted border.
pub fn card_attention(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        border: outline(mix(p.border, p.warning, 0.6), radius::MD),
        ..card(theme)
    }
}

pub fn panel(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface.into()),
        border: Border {
            color: p.border,
            width: BORDER_WIDTH,
            radius: border::Radius::default().top_left(radius::MD),
        },
        ..container::Style::default()
    }
}

fn chrome(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.bg.into()),
        ..container::Style::default()
    }
}

pub fn topbar(theme: &Theme) -> container::Style {
    chrome(theme)
}

pub fn sidebar(theme: &Theme) -> container::Style {
    chrome(theme)
}

pub fn bottombar(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface_raised.into()),
        border: outline(p.border, 0.0),
        ..container::Style::default()
    }
}

pub fn window(theme: &Theme) -> container::Style {
    chrome(theme)
}

pub fn sunken(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface.into()),
        border: outline(p.border, radius::MD),
        ..container::Style::default()
    }
}

/// One line of a diff: a faint tint of `tone`, none for `Neutral` and `Muted`.
pub fn diff_line(tone: Tone) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = tokens(theme);
        let background = match tone {
            Tone::Neutral | Tone::Muted => None,
            other => Some(mix(p.surface, other.color(&p), 0.14).into()),
        };
        container::Style {
            text_color: Some(p.text),
            background,
            ..container::Style::default()
        }
    }
}

pub fn dialog(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface_raised.into()),
        border: outline(mix(p.border, p.text_muted, 0.25), radius::MD),
        ..container::Style::default()
    }
}

pub fn backdrop(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        background: Some(Color { a: 0.6, ..p.bg }.into()),
        ..container::Style::default()
    }
}

pub fn toast(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface_raised.into()),
        border: outline(mix(p.border, p.text_muted, 0.45), radius::MD),
        ..container::Style::default()
    }
}

pub fn banner(tone: Tone) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = tokens(theme);
        let color = tone.color(&p);
        container::Style {
            text_color: Some(p.text),
            background: Some(mix(p.surface, color, 0.12).into()),
            border: Border {
                color: mix(p.border, color, 0.45),
                width: BORDER_WIDTH,
                radius: radius::SM.into(),
            },
            ..container::Style::default()
        }
    }
}

pub fn tag(tone: Tone) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = tokens(theme);
        let color = match tone {
            Tone::Neutral | Tone::Muted => p.text_muted,
            other => other.color(&p),
        };
        container::Style {
            text_color: Some(match tone {
                Tone::Neutral => p.text,
                Tone::Muted => p.text_muted,
                _ => mix(color, p.text, 0.4),
            }),
            background: Some(mix(p.surface_raised, color, 0.14).into()),
            border: outline(mix(p.surface_raised, color, 0.35), radius::PILL),
            ..container::Style::default()
        }
    }
}

pub fn pill(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface.into()),
        border: outline(p.border, radius::PILL),
        ..container::Style::default()
    }
}

pub fn dot(tone: Tone) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = tokens(theme);
        container::Style {
            background: Some(tone.color(&p).into()),
            border: border::rounded(radius::PILL),
            ..container::Style::default()
        }
    }
}

pub fn badge(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.accent_text),
        background: Some(p.accent.into()),
        border: border::rounded(radius::PILL),
        ..container::Style::default()
    }
}

pub fn marker(changed: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = tokens(theme);
        container::Style {
            background: changed.then(|| p.accent.into()),
            border: border::rounded(radius::PILL),
            ..container::Style::default()
        }
    }
}

pub fn key_cap(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface.into()),
        border: Border {
            color: mix(p.border, p.text_muted, 0.3),
            width: BORDER_WIDTH,
            radius: radius::SM.into(),
        },
        shadow: Shadow {
            color: p.border,
            offset: Vector::new(0.0, BORDER_WIDTH),
            blur_radius: 0.0,
        },
        snap: true,
    }
}

pub fn icon_frame(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text_muted),
        background: Some(p.surface_raised.into()),
        border: outline(p.border, radius::PILL),
        ..container::Style::default()
    }
}

pub fn table_header(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text_muted),
        background: Some(p.surface.into()),
        border: Border {
            color: p.border,
            width: BORDER_WIDTH,
            radius: border::Radius::default()
                .top_left(radius::MD)
                .top_right(radius::MD),
        },
        ..container::Style::default()
    }
}

pub fn segments(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text),
        background: Some(p.surface.into()),
        border: outline(p.border, radius::SM),
        ..container::Style::default()
    }
}

// Buttons

pub fn button_style(kind: ButtonKind) -> fn(&Theme, button::Status) -> button::Style {
    match kind {
        ButtonKind::Primary => primary,
        ButtonKind::Secondary => secondary,
        ButtonKind::Ghost => ghost,
        ButtonKind::Danger => danger,
        ButtonKind::DangerGhost => danger_ghost,
    }
}

fn filled(base: Color, text_color: Color, p: &Palette, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Active => base,
        button::Status::Hovered => mix(base, Color::WHITE, 0.12),
        button::Status::Pressed => pressed_of(base, p),
        button::Status::Disabled => p.surface_raised,
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled {
            p.text_muted
        } else {
            text_color
        },
        border: outline(
            if status == button::Status::Disabled {
                p.border
            } else {
                background
            },
            radius::SM,
        ),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let p = tokens(theme);
    filled(p.accent, p.accent_text, &p, status)
}

pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = tokens(theme);
    filled(danger_fill(&p), Color::WHITE, &p, status)
}

fn danger_fill(p: &Palette) -> Color {
    mix(p.danger, Color::BLACK, 0.18)
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let p = tokens(theme);
    let background = match status {
        button::Status::Active | button::Status::Disabled => p.surface_raised,
        button::Status::Hovered => hover_of(p.surface_raised, &p),
        button::Status::Pressed => p.surface,
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled {
            p.text_muted
        } else {
            p.text
        },
        border: outline(mix(p.border, p.text_muted, 0.2), radius::SM),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = tokens(theme);
    let background = match status {
        button::Status::Active | button::Status::Disabled => None,
        button::Status::Hovered => Some(Background::from(mix(p.surface_raised, p.text, 0.06))),
        button::Status::Pressed => Some(Background::from(p.border)),
    };
    button::Style {
        background,
        text_color: if status == button::Status::Disabled {
            p.text_muted
        } else {
            p.text
        },
        border: border::rounded(radius::SM),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn danger_ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = tokens(theme);
    let style = ghost(theme, status);
    if status == button::Status::Disabled {
        return style;
    }
    button::Style {
        text_color: mix(p.danger, p.text, 0.15),
        ..style
    }
}

pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = tokens(theme);
        let background = match (active, status) {
            (true, _) => Some(Background::from(p.surface_raised)),
            (false, button::Status::Hovered) => Some(Background::from(mix(p.bg, p.text, 0.05))),
            (false, button::Status::Pressed) => Some(Background::from(p.surface)),
            (false, _) => None,
        };
        button::Style {
            background,
            text_color: if active { p.text } else { p.text_muted },
            border: if active {
                outline(p.border, radius::SM)
            } else {
                border::rounded(radius::SM)
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn segment(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = tokens(theme);
        let background = match (active, status) {
            (true, _) => Some(Background::from(p.surface_raised)),
            (false, button::Status::Hovered) => {
                Some(Background::from(mix(p.surface, p.text, 0.05)))
            }
            (false, _) => None,
        };
        button::Style {
            background,
            text_color: if active { p.text } else { p.text_muted },
            border: if active {
                outline(mix(p.border, p.text_muted, 0.25), radius::SM)
            } else {
                border::rounded(radius::SM)
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn row_button(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = tokens(theme);
        let background = match (selected, status) {
            (true, _) => Some(Background::from(mix(p.surface_raised, p.text, 0.08))),
            (false, button::Status::Hovered) => {
                Some(Background::from(mix(p.surface_raised, p.text, 0.04)))
            }
            (false, _) => None,
        };
        button::Style {
            background,
            text_color: p.text,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

// Text

pub fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).text_muted),
    }
}

pub fn title(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).text),
    }
}

pub fn caption(theme: &Theme) -> text::Style {
    muted(theme)
}

pub fn accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).accent),
    }
}

pub fn toned(tone: Tone) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(tone.color(&tokens(theme))),
    }
}

pub fn on_accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).accent_text),
    }
}

// Icons

pub fn icon_tint(tone: Tone) -> impl Fn(&Theme, svg::Status) -> svg::Style {
    move |theme, _| svg::Style {
        color: Some(tone.color(&tokens(theme))),
    }
}

pub fn icon_on_accent(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(tokens(theme).accent_text),
    }
}

// Controls

pub fn text_field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = tokens(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => p.text_muted,
        text_input::Status::Hovered => mix(p.border, p.text_muted, 0.5),
        text_input::Status::Active | text_input::Status::Disabled => {
            mix(p.border, p.text_muted, 0.2)
        }
    };
    text_input::Style {
        background: (if status == text_input::Status::Disabled {
            p.surface_raised
        } else {
            p.surface
        })
        .into(),
        border: outline(border_color, radius::SM),
        icon: p.text_muted,
        placeholder: p.text_muted,
        value: p.text,
        selection: mix(p.surface, p.accent, 0.35),
    }
}

pub fn toggle(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let p = tokens(theme);
    let (on, hovered, disabled) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    let track = match (on, disabled) {
        (_, true) => p.border,
        (true, false) if hovered => mix(p.accent, Color::WHITE, 0.12),
        (true, false) => p.accent,
        (false, false) => mix(p.border, p.text_muted, if hovered { 0.45 } else { 0.25 }),
    };
    let knob = if on && !disabled {
        p.surface_raised
    } else {
        p.text
    };
    toggler::Style {
        background: track.into(),
        background_border_width: 0.0,
        background_border_color: track,
        foreground: knob.into(),
        foreground_border_width: 0.0,
        foreground_border_color: knob,
        text_color: Some(p.text),
        border_radius: None,
        padding_ratio: TOGGLER_PADDING_RATIO,
    }
}

pub fn check(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = tokens(theme);
    let (checked, hovered) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true),
        checkbox::Status::Disabled { is_checked } => (is_checked, false),
    };
    let background = if checked { p.accent } else { p.surface };
    checkbox::Style {
        background: background.into(),
        icon_color: p.accent_text,
        border: outline(
            if checked {
                p.accent
            } else if hovered {
                p.text_muted
            } else {
                mix(p.border, p.text_muted, 0.3)
            },
            radius::SM / 2.0,
        ),
        text_color: Some(p.text),
    }
}

pub fn range(theme: &Theme, status: slider::Status) -> slider::Style {
    let p = tokens(theme);
    let handle = match status {
        slider::Status::Active => p.text,
        slider::Status::Hovered | slider::Status::Dragged => mix(p.text, Color::WHITE, 0.3),
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (p.text_muted.into(), p.border.into()),
            width: SLIDER_RAIL_WIDTH,
            border: border::rounded(radius::PILL),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: SLIDER_HANDLE_RADIUS,
            },
            background: handle.into(),
            border_width: BORDER_WIDTH,
            border_color: p.surface_raised,
        },
    }
}

pub fn pick(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let p = tokens(theme);
    let border_color = match status {
        pick_list::Status::Active => mix(p.border, p.text_muted, 0.2),
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
            mix(p.border, p.text_muted, 0.5)
        }
    };
    pick_list::Style {
        text_color: p.text,
        placeholder_color: p.text_muted,
        handle_color: p.text_muted,
        background: p.surface_raised.into(),
        border: outline(border_color, radius::SM),
    }
}

pub fn pick_menu(theme: &Theme) -> menu::Style {
    let p = tokens(theme);
    menu::Style {
        background: p.surface_raised.into(),
        border: outline(mix(p.border, p.text_muted, 0.3), radius::SM),
        text_color: p.text,
        selected_text_color: p.text,
        selected_background: mix(p.surface_raised, p.text, 0.1).into(),
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.25),
            offset: Vector::new(0.0, space::XS),
            blur_radius: space::LG,
        },
    }
}

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = tokens(theme);
    let hovered = matches!(
        status,
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
    );
    let scroller = if hovered {
        p.text_muted
    } else {
        mix(p.border, p.text_muted, 0.3)
    };
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: scroller.into(),
            border: border::rounded(radius::PILL),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: p.surface_raised.into(),
            border: outline(p.border, radius::PILL),
            shadow: Shadow::default(),
            icon: p.text,
        },
    }
}

pub const SCROLLBAR_WIDTH: f32 = SCROLLER_WIDTH;

pub fn meter(theme: &Theme) -> progress_bar::Style {
    let p = tokens(theme);
    progress_bar::Style {
        background: p.border.into(),
        bar: p.text_muted.into(),
        border: border::rounded(radius::PILL),
    }
}

pub fn divider(theme: &Theme) -> rule::Style {
    let p = tokens(theme);
    rule::Style {
        color: p.border,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub const DIVIDER: f32 = BORDER_WIDTH;

/// A switch for a setting the file does not have: it shows the game default without the accent.
pub fn toggle_unset(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let p = tokens(theme);
    let hovered = matches!(status, toggler::Status::Hovered { .. });
    let track = mix(p.border, p.text_muted, if hovered { 0.25 } else { 0.0 });
    let knob = mix(p.text_muted, p.surface_raised, 0.35);
    toggler::Style {
        background: track.into(),
        background_border_color: track,
        foreground: knob.into(),
        foreground_border_color: knob,
        ..toggle(theme, status)
    }
}

/// A slider for a setting the file does not have: muted rail and handle.
pub fn range_unset(theme: &Theme, status: slider::Status) -> slider::Style {
    let p = tokens(theme);
    let base = range(theme, status);
    slider::Style {
        rail: slider::Rail {
            backgrounds: (p.border.into(), p.border.into()),
            ..base.rail
        },
        handle: slider::Handle {
            background: p.text_muted.into(),
            ..base.handle
        },
    }
}

/// A multi-line text editor, styled like [`text_field`].
pub fn editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let field = text_field(
        theme,
        match status {
            text_editor::Status::Active => text_input::Status::Active,
            text_editor::Status::Hovered => text_input::Status::Hovered,
            text_editor::Status::Focused { is_hovered } => {
                text_input::Status::Focused { is_hovered }
            }
            text_editor::Status::Disabled => text_input::Status::Disabled,
        },
    );
    text_editor::Style {
        background: field.background,
        border: field.border,
        placeholder: field.placeholder,
        value: field.value,
        selection: field.selection,
    }
}

pub mod binds {
    pub const MAP_LIST_WIDTH: f32 = 240.0;
    pub const CAPTURE_HEIGHT: f32 = 72.0;
    pub const QUICK_PICK_WIDTH: f32 = 40.0;
    pub const EDIT_SIDE_WIDTH: f32 = 320.0;
    pub const ACTION_PORTION: u16 = 2;
    pub const DEVICE_PORTION: u16 = 1;
}

/// A clickable bind cell in the keybinds table.
pub fn bind_cell(
    selected: bool,
    conflict: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = tokens(theme);
        let resting = conflict.then(|| mix(p.surface_raised, p.warning, 0.12));
        let background = match (selected, status) {
            (true, _) => Some(mix(p.surface_raised, p.accent, 0.14)),
            (false, button::Status::Hovered) => {
                Some(mix(resting.unwrap_or(p.surface_raised), p.text, 0.06))
            }
            (false, _) => resting,
        };
        button::Style {
            background: background.map(Background::from),
            text_color: p.text,
            border: if selected {
                outline(p.accent, radius::SM)
            } else {
                border::rounded(radius::SM)
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// The box that listens for a key or mouse button.
pub fn capture_box(listening: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = tokens(theme);
        let (background, edge) = if listening {
            (mix(p.surface, p.accent, 0.08), p.accent)
        } else {
            (p.surface, p.border)
        };
        container::Style {
            text_color: Some(p.text),
            background: Some(background.into()),
            border: outline(edge, radius::MD),
            ..container::Style::default()
        }
    }
}

// Screenshots. Overlays sit on pictures, so they keep dark colours in both themes.

const IMAGE_STAGE: Color = Color::from_rgb8(0x07, 0x09, 0x0C);
const OVERLAY_ALPHA: f32 = 0.72;

pub fn shot_tile(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = tokens(theme);
        let ring = match (selected, status) {
            (true, _) => p.accent,
            (false, button::Status::Hovered) => mix(p.border, p.text_muted, 0.6),
            (false, _) => p.border,
        };
        button::Style {
            background: Some(p.surface.into()),
            text_color: p.text,
            border: Border {
                color: ring,
                width: if selected {
                    layout::SHOT_TILE_RING
                } else {
                    BORDER_WIDTH
                },
                radius: radius::SM.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn shot_placeholder(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(p.text_muted),
        background: Some(p.surface.into()),
        ..container::Style::default()
    }
}

pub fn shot_strip(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(DARK.text),
        background: Some(
            Color {
                a: OVERLAY_ALPHA,
                ..IMAGE_STAGE
            }
            .into(),
        ),
        ..container::Style::default()
    }
}

pub fn shot_chip(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(DARK.text),
        background: Some(
            Color {
                a: OVERLAY_ALPHA,
                ..IMAGE_STAGE
            }
            .into(),
        ),
        border: border::rounded(radius::SM),
        ..container::Style::default()
    }
}

pub fn on_image(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(DARK.text),
    }
}

pub fn image_stage(theme: &Theme) -> container::Style {
    let p = tokens(theme);
    container::Style {
        text_color: Some(DARK.text_muted),
        background: Some(IMAGE_STAGE.into()),
        border: outline(mix(IMAGE_STAGE, p.border, 0.5), radius::MD),
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: Color, b: Color) -> f32 {
        a.relative_contrast(b)
    }

    #[test]
    fn muted_text_is_readable_on_every_surface() {
        for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
            let p = palette(choice);
            for surface in [p.bg, p.surface, p.surface_raised] {
                assert!(contrast(p.text_muted, surface) >= 4.5, "{choice:?}");
                assert!(contrast(p.text, surface) >= 7.0, "{choice:?}");
            }
            assert!(contrast(p.accent_text, p.accent) >= 4.5, "{choice:?}");
            assert!(contrast(Color::WHITE, danger_fill(&p)) >= 4.5, "{choice:?}");
        }
    }

    #[test]
    fn tokens_follow_the_theme() {
        assert_eq!(tokens(&theme(ThemeChoice::Dark)), DARK);
        assert_eq!(tokens(&theme(ThemeChoice::Light)), LIGHT);
    }
}
