#[path = "../src/testkit.rs"]
mod testkit;

use iced::widget::{Space, column, container, row};
use iced::{Element, Length, Size};
use iced_test::simulator::Simulator;
use sc_app::icons;
use sc_app::screen::Toast;
use sc_app::theme::{self, ButtonKind, Tone, layout, space};
use sc_app::widgets::*;
use sc_core::backup::SnapshotId;
use sc_core::settings::ThemeChoice;

#[derive(Debug, Clone)]
struct Noop;

const SIZE: Size = Size::new(1280.0, 1180.0);

fn controls() -> Element<'static, Noop> {
    let buttons = row![
        button("Apply", ButtonKind::Primary, Some(Noop)),
        button("Discard", ButtonKind::Secondary, Some(Noop)),
        button("Details", ButtonKind::Ghost, Some(Noop)),
        button("Delete snapshot", ButtonKind::Danger, Some(Noop)),
        button("Disabled", ButtonKind::Primary, None),
        icon_button(icons::LAUNCH, "Launch", ButtonKind::Primary, Some(Noop)),
        icon_button(
            icons::FOLDER,
            "Choose folder",
            ButtonKind::Secondary,
            Some(Noop)
        ),
        icon_only(icons::CLOSE, Some(Noop)),
    ]
    .spacing(space::SM)
    .align_y(iced::Alignment::Center);

    let settings = rows([
        setting_row(
            "Vertical sync",
            Some("Caps the frame rate to the display refresh rate."),
            false,
            toggle(true, |_| Noop),
        ),
        setting_row("Motion blur", None, true, toggle(false, |_| Noop)),
        field_row(
            "Field of view",
            Some("Degrees, horizontal."),
            true,
            Some("90".to_string()),
            row![
                slider(60u32..=120u32, 100, |_| Noop),
                label("100").width(layout::VALUE_WIDTH)
            ]
            .spacing(space::MD)
            .align_y(iced::Alignment::Center),
        ),
        setting_row(
            "Quality preset",
            Some("Applies to every graphics setting below."),
            false,
            pick_list(
                vec!["Low", "Medium", "High", "Very high"],
                Some("High"),
                |_| Noop,
            )
            .width(layout::CONTROL_WIDTH),
        ),
        field_row(
            "Launch command",
            Some("Runs instead of the detected launcher."),
            true,
            Some("empty".to_string()),
            text_field(
                "lutris lutris:rungame/star-citizen",
                "lutris rungame",
                |_| Noop,
            )
            .width(layout::WIDE_CONTROL_WIDTH),
        ),
        setting_row(
            "Snapshots to keep",
            None,
            false,
            number_field("50", |_| Noop),
        ),
        setting_row(
            "Show hints",
            None,
            false,
            checkbox("Enabled", true, |_| Noop),
        ),
        setting_row(
            "Theme",
            None,
            false,
            segmented(vec![("Dark", true, Noop), ("Light", false, Noop)]),
        ),
    ]);

    page_fixed(
        "Widget kit",
        Some("Every kit widget on one page, part one."),
        vec![
            search_field("", "Search settings", |_| Noop),
            button("Apply", ButtonKind::Primary, Some(Noop)),
        ],
        column![
            section("Buttons", container(buttons).padding(space::LG)),
            section("Setting rows", settings),
            row![
                column![
                    heading("TYPE SCALE"),
                    title("Page title"),
                    strong("Strong body"),
                    label("Body label"),
                    muted("Muted body"),
                    hint("Hint and caption"),
                    display("4.2 GB"),
                ]
                .spacing(space::SM)
                .width(Length::Fill),
                column![
                    heading("INLINE"),
                    row![
                        tag("Current", Tone::Success),
                        tag("Unmatched", Tone::Warning),
                        tag("Interrupted", Tone::Danger),
                        tag("Changed", Tone::Accent),
                        tag("Found automatically", Tone::Muted),
                        tag("Selected", Tone::Neutral),
                    ]
                    .spacing(space::SM),
                    row![
                        key_cap("LCtrl"),
                        key_cap("Shift"),
                        key_cap("F"),
                        key_cap("Mouse 4"),
                        badge(3),
                        status_pill("Game closed", Tone::Success),
                        status_pill("Game running", Tone::Warning),
                    ]
                    .spacing(space::SM)
                    .align_y(iced::Alignment::Center),
                    progress(0.62, "Copying shader cache"),
                    row![
                        icon(icons::CHECK, layout::ICON),
                        icon_toned(icons::WARNING, layout::ICON, Tone::Warning),
                        icon_toned(icons::INFO, layout::ICON, Tone::Muted),
                        change_marker(true),
                    ]
                    .spacing(space::MD),
                ]
                .spacing(space::MD)
                .width(Length::Fill),
            ]
            .spacing(space::XL),
        ]
        .spacing(space::XL),
    )
}

fn feedback(toasts: &[Toast]) -> Element<'_, Noop> {
    const COLUMNS: [(&str, Length); 3] = [
        ("Action", Length::FillPortion(3)),
        ("Binding", Length::FillPortion(2)),
        ("State", Length::Fixed(120.0)),
    ];
    let rows_data = [
        (
            "Fire weapon group 1",
            "Mouse 1",
            Some(("Current", Tone::Success)),
        ),
        ("Cycle target forward", "T", Some(("Changed", Tone::Accent))),
        ("Toggle landing gear", "N", None),
    ];
    let mut table = column![table_header(&COLUMNS)];
    for (index, (action, binding, state)) in rows_data.into_iter().enumerate() {
        let state_cell: Element<'_, Noop> = match state {
            Some((text, tone)) => tag(text, tone),
            None => hint("Default").into(),
        };
        table = table.push(table_row(
            vec![
                (label(action).into(), COLUMNS[0].1),
                (key_cap(binding), COLUMNS[1].1),
                (state_cell, COLUMNS[2].1),
            ],
            index == 1,
            Some(Noop),
        ));
    }
    let list_state: &'static VirtualListState = Box::leak(Box::default());
    let list = virtual_list(
        list_state,
        500,
        layout::TABLE_ROW_HEIGHT,
        |_| Noop,
        |index| {
            table_row(
                vec![(
                    label(format!("Screenshot {index:04}.jpg")).into(),
                    Length::Fill,
                )],
                index == 2,
                Some(Noop),
            )
        },
    );

    page_fixed(
        "Widget kit",
        Some("Every kit widget on one page, part two."),
        Vec::new(),
        column![
            banner(
                ToastKind::Warning,
                "Star Citizen is running. Files are read-only until it closes.",
                Some(("It is not running, unlock", Noop)),
            ),
            banner(
                ToastKind::Error,
                "attributes.xml changed on disk since it was loaded.",
                None
            ),
            banner(
                ToastKind::Info,
                "A new StarStrings release is available.",
                Some(("Update", Noop))
            ),
            row![
                stat_card(
                    "Config",
                    "12 changes",
                    Some("user.cfg and attributes.xml"),
                    Some(("Open", Noop))
                ),
                stat_card(
                    "Shader cache",
                    "4.2 GB",
                    Some("Last cleared 12 days ago"),
                    None
                ),
                stat_card("Snapshots", "38", None, None),
            ]
            .spacing(space::LG),
            section("Table", table),
            row![
                container(split(
                    list,
                    layout::SPLIT_LEFT,
                    column![
                        title("Screenshot 0002.jpg"),
                        hint("3840 x 2160, 4.1 MB"),
                        Space::new().height(space::SM),
                        empty_state(
                            "Nothing selected",
                            "Pick a screenshot to see it here.",
                            Some(("Open folder", Noop))
                        ),
                    ]
                ))
                .height(320)
                .width(Length::FillPortion(3)),
                container(toast_stack(toasts, |_| Noop, |_| Noop))
                    .width(Length::FillPortion(2))
                    .align_right(Length::FillPortion(2)),
            ]
            .spacing(space::LG),
        ]
        .spacing(space::LG),
    )
}

fn frame<'a>(content: Element<'a, Noop>) -> Element<'a, Noop> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel)
        .into()
}

#[test]
fn gallery_renders_every_widget_in_both_themes() {
    let toasts = vec![
        Toast::success("Applied 3 changes to user.cfg").with_undo(SnapshotId("a".into())),
        Toast::info("Starting the RSI Launcher"),
        Toast::error("Could not write attributes.xml"),
    ];
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let theme = theme::theme(choice);
        let mut ui = Simulator::with_size(sc_app::settings(), SIZE, frame(controls()));
        testkit::snapshot(&mut ui, &theme, &format!("gallery-controls-{name}"));
        let mut ui = Simulator::with_size(sc_app::settings(), SIZE, frame(feedback(&toasts)));
        testkit::snapshot(&mut ui, &theme, &format!("gallery-feedback-{name}"));
        let dialog = iced::widget::stack![
            frame(controls()),
            confirm(
                "Start the launcher?",
                "star-config will run:\n/home/zoe/Games/star-citizen/sc-launch.sh\n\nThis script closes every program running in this Wine prefix before it starts the launcher.",
                vec![
                    ("Launch", ButtonKind::Primary, Noop),
                    ("Cancel", ButtonKind::Secondary, Noop)
                ],
            ),
        ];
        let mut ui = Simulator::with_size(sc_app::settings(), SIZE, dialog);
        testkit::snapshot(&mut ui, &theme, &format!("gallery-confirm-{name}"));
    }
}
