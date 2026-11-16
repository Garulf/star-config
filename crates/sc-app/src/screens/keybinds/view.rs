use super::inputs::{
    AXES, Typed, captures, judge, quick_buttons, quick_hats, quick_label, slot_label,
};
use super::model::{Cell, CellRef, Line, MapEntry, Model, Row as BindRow};
use super::source::{Failure, UNRECOGNISED};
use super::{
    ADD_CAPTION, AddForm, CAPTURE_PROMPT, Doc, EMPTY_BODY, EMPTY_TITLE, Edit, Message,
    RESET_CAPTION, SaveForm, State, SwapForm, UNBIND_CAPTION, add_input,
};
use crate::icons;
use crate::screen::{Screen, ToastKind};
use crate::theme::{self, ButtonKind, FONT_MONO, FONT_SEMIBOLD, Tone, binds, layout, size, space};
use crate::widgets::{
    banner, button, change_marker, divider, empty_state, heading, hint, icon_only, icon_toned,
    key_cap, label, muted, page_fixed, pick_list, search_field, table_header, tag, text_field,
    virtual_list,
};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    self, Column, Row, Space, column, container, mouse_area, row, scrollable, text,
};
use iced::{Alignment, Element, Length, Padding};
use sc_core::actionmaps::{DeviceKind, Slot, layout_file_name};

const ROW_HEIGHT: f32 = layout::ROW_MIN_HEIGHT;
const CELL_GAP: f32 = space::XS;
const TIGHT: f32 = space::XS / 2.0;
const CELL_PADDING: Padding = Padding {
    top: space::XS,
    right: space::SM,
    bottom: space::XS,
    left: space::SM,
};
const ROW_SIDES: Padding = Padding {
    top: 0.0,
    right: space::SM,
    bottom: 0.0,
    left: space::SM,
};
const LIST_ITEM_PADDING: Padding = Padding {
    top: space::SM,
    right: space::LG,
    bottom: space::SM,
    left: space::LG,
};
const CHIP_PADDING: Padding = Padding {
    top: TIGHT,
    right: space::SM,
    bottom: TIGHT,
    left: space::SM,
};
const NO_MATCH: &str = "No binds match. Clear the search or pick another action map.";
const PICK_A_CELL: &str = "Choose a cell to change its bind.";

fn action_width() -> Length {
    Length::FillPortion(binds::ACTION_PORTION)
}

fn device_width() -> Length {
    Length::FillPortion(binds::DEVICE_PORTION)
}

impl State {
    pub(super) fn screen(&self) -> Element<'_, Message> {
        let title = Screen::Keybinds.label();
        match (&self.doc, &self.model) {
            (Doc::Empty, _) => page_fixed(
                title,
                None,
                Vec::new(),
                empty_state(EMPTY_TITLE, EMPTY_BODY, None),
            ),
            (Doc::Failed(failure), _) => page_fixed(title, None, Vec::new(), self.failed(failure)),
            (Doc::Ready { .. } | Doc::Away, Some(model)) => self.main(model),
            _ => page_fixed(
                title,
                None,
                Vec::new(),
                container(hint("Reading your keybinds...")).center(Length::Fill),
            ),
        }
    }

    fn main<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        let mut body = column![self.toolbar(model)]
            .spacing(space::LG)
            .height(Length::Fill);
        if !self.writable() && matches!(self.doc, Doc::Ready { .. }) {
            body = body.push(banner(ToastKind::Warning, UNRECOGNISED, None));
        }
        body = body.push(
            row![
                container(self.map_list(model))
                    .width(binds::MAP_LIST_WIDTH)
                    .height(Length::Fill)
                    .style(theme::card),
                container(self.table_area(model))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(theme::card),
            ]
            .spacing(space::LG)
            .height(Length::Fill),
        );
        page_fixed(
            Screen::Keybinds.label(),
            Some(model.devices.as_str()),
            vec![search_field(
                &self.search,
                "Search actions and inputs",
                Message::Search,
            )],
            body,
        )
    }

    fn source_pick(&self) -> Element<'_, Message> {
        let selected = self
            .sources
            .iter()
            .find(|choice| Some(&choice.source) == self.source.as_ref())
            .cloned();
        pick_list(self.sources.clone(), selected, Message::PickSource)
            .width(layout::CONTROL_WIDTH)
            .into()
    }

    fn toolbar<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        let writable = self.writable();
        let mut bar = row![self.source_pick()]
            .spacing(space::SM)
            .align_y(Alignment::Center);
        if self.source.as_ref().is_some_and(super::Source::is_layout) {
            bar = bar.push(button(
                "Save as layout...",
                ButtonKind::Secondary,
                self.can_save_layout().then_some(Message::OpenSaveAs),
            ));
        }
        bar = bar
            .push(Space::new().width(Length::Fill))
            .push(self.conflicts_chip(model.conflicts));
        if model.joysticks().len() >= 2 {
            bar = bar.push(button(
                "Swap joysticks",
                ButtonKind::Secondary,
                writable.then_some(Message::OpenSwap),
            ));
        }
        bar.push(button(
            "Add a bind by name",
            ButtonKind::Secondary,
            writable.then_some(Message::OpenAdd),
        ))
        .into()
    }

    fn conflicts_chip(&self, count: usize) -> Element<'_, Message> {
        let tone = if count > 0 {
            Tone::Warning
        } else {
            Tone::Muted
        };
        let kind = if self.conflicts_only {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        };
        widget::button(
            row![
                icon_toned(icons::WARNING, layout::ICON, tone),
                text("Conflicts").size(size::BODY),
                tag(count.to_string(), tone),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center)
            .height(Length::Fill),
        )
        .height(layout::CONTROL_HEIGHT)
        .padding(Padding {
            left: space::MD,
            right: space::SM,
            ..Padding::ZERO
        })
        .style(theme::button_style(kind))
        .on_press(Message::ToggleConflicts)
        .into()
    }

    fn map_list<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        let mut items = Column::new().push(map_item(
            "All",
            model.total,
            self.map.is_none(),
            Message::PickMap(None),
        ));
        for entry in &model.maps {
            items = items.push(map_item(
                &entry.label,
                entry.count,
                self.map.as_deref() == Some(entry.name.as_str()),
                Message::PickMap(Some(entry.name.clone())),
            ));
        }
        column![
            table_header(&[("Action maps", Length::Fill)]),
            vertical_scroll(items.padding(Padding {
                top: space::XS,
                ..Padding::ZERO
            })),
        ]
        .into()
    }

    fn table_area<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        let table: Element<'a, Message> = if self.lines.is_empty() {
            container(hint(NO_MATCH)).center(Length::Fill).into()
        } else {
            virtual_list(
                &self.list,
                self.lines.len(),
                ROW_HEIGHT,
                Message::Scrolled,
                move |index| self.line(model, self.lines[index]),
            )
        };
        column![
            header_row(model),
            container(table).height(Length::Fill),
            divider(),
            container(self.panel_view(model))
                .padding(space::LG)
                .width(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    fn line<'a>(&'a self, model: &'a Model, line: Line) -> Element<'a, Message> {
        match line {
            Line::Group(index) => group_line(&model.maps[index]),
            Line::Action(index) => self.action_row(model, &model.rows[index]),
        }
    }

    fn action_row<'a>(&'a self, model: &'a Model, bind: &'a BindRow) -> Element<'a, Message> {
        let mut cells: Vec<Element<'a, Message>> = vec![
            container(
                column![
                    label(bind.label.as_str()).wrapping(text::Wrapping::None),
                    hint(bind.action.as_str()).wrapping(text::Wrapping::None),
                ]
                .spacing(TIGHT),
            )
            .padding(CELL_PADDING)
            .width(action_width())
            .clip(true)
            .into(),
        ];
        for (slot, cell) in model.columns.iter().zip(&bind.cells) {
            cells.push(self.bind_cell(bind, *slot, cell));
        }
        container(
            Row::with_children(cells)
                .spacing(CELL_GAP)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: space::XS,
            bottom: space::XS,
            ..ROW_SIDES
        })
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    fn bind_cell<'a>(
        &'a self,
        bind: &'a BindRow,
        slot: Slot,
        cell: &'a Cell,
    ) -> Element<'a, Message> {
        let target = CellRef {
            map: bind.map.clone(),
            action: bind.action.clone(),
            slot,
        };
        let selected = matches!(&self.panel, super::Panel::Edit(edit) if edit.cell == target);
        widget::button(cell_content(cell))
            .padding(CELL_PADDING)
            .width(device_width())
            .height(Length::Fill)
            .style(theme::bind_cell(selected, cell.conflict))
            .on_press_maybe(self.writable().then_some(Message::Select(target)))
            .into()
    }

    fn panel_view<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        match &self.panel {
            super::Panel::Closed => {
                let note = if self.writable() {
                    PICK_A_CELL
                } else {
                    UNRECOGNISED
                };
                hint(note).into()
            }
            super::Panel::Edit(edit) => edit_panel(model, edit),
            super::Panel::Add(form) => self.add_panel(model, form),
            super::Panel::Swap(form) => self.swap_panel(model, form),
            super::Panel::SaveAs(form) => self.save_panel(form),
        }
    }

    fn add_panel<'a>(&'a self, model: &'a Model, form: &'a AddForm) -> Element<'a, Message> {
        let maps: Vec<String> = model.maps.iter().map(|entry| entry.name.clone()).collect();
        let ready = add_input(form).is_some();
        let error = form
            .slot
            .as_ref()
            .map(|choice| judge(choice.slot, &form.input));
        let first = row![
            labelled(
                "Action map",
                text_field("spaceship_mining", &form.map, Message::AddMap).width(Length::Fill),
            ),
            labelled(
                "Or pick one from this file",
                pick_list(maps, None::<String>, Message::AddMap)
                    .placeholder("Choose an action map")
                    .width(Length::Fill),
            ),
            labelled(
                "Action",
                text_field("v_toggle_mining_mode", &form.action, Message::AddAction)
                    .width(Length::Fill),
            ),
        ]
        .spacing(space::MD);
        let second = row![
            labelled(
                "Device",
                pick_list(self.slot_choices(), form.slot.clone(), Message::AddSlot)
                    .width(Length::Fill),
            ),
            labelled(
                "Input",
                text_field("button5", &form.input, Message::AddInput)
                    .on_submit_maybe(ready.then_some(Message::AddBind))
                    .width(Length::Fill),
            ),
            column![
                Space::new().height(size::CAPTION),
                button(
                    "Add bind",
                    ButtonKind::Primary,
                    ready.then_some(Message::AddBind)
                ),
            ]
            .spacing(space::XS)
            .width(Length::Fill),
        ]
        .spacing(space::MD);
        let mut content =
            column![panel_title("Add a bind by name"), first, second].spacing(space::MD);
        if let Some(Typed::Invalid(error)) = error {
            content = content.push(error_text(error));
        }
        content.push(hint(ADD_CAPTION)).into()
    }

    fn swap_panel<'a>(&'a self, model: &'a Model, form: &'a SwapForm) -> Element<'a, Message> {
        let choices = self.joystick_choices();
        let (summary, ready) = match (&form.a, &form.b) {
            (Some(a), Some(b)) if a.slot != b.slot => (
                format!(
                    "Swaps every {} bind with {}, {}",
                    slot_label(a.slot),
                    slot_label(b.slot),
                    binds_text(model.joystick_binds(a.slot.instance, b.slot.instance))
                ),
                true,
            ),
            _ => ("Pick two different joysticks.".to_string(), false),
        };
        column![
            panel_title("Swap joysticks"),
            row![
                pick_list(choices.clone(), form.a.clone(), Message::SwapA)
                    .width(layout::CONTROL_WIDTH),
                muted("and"),
                pick_list(choices, form.b.clone(), Message::SwapB)
                    .width(layout::CONTROL_WIDTH),
                button("Swap", ButtonKind::Primary, ready.then_some(Message::Swap)),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center),
            column![
                label(summary),
                hint("Use this when Windows or Wine numbers your sticks in a different order than before."),
            ]
            .spacing(space::XS),
        ]
        .spacing(space::MD)
        .into()
    }

    fn save_panel<'a>(&'a self, form: &'a SaveForm) -> Element<'a, Message> {
        let name = form.name.trim();
        let file = layout_file_name(name);
        let ready = file.is_ok() && !self.saving;
        let caption = match &file {
            Ok(file) => format!(
                "Writes {file} into Controls/Mappings, with your unsaved changes. This works while the game runs."
            ),
            Err(_) => "Saves these binds, with your unsaved changes, as a new layout in Controls/Mappings. This works while the game runs.".to_string(),
        };
        let mut content = column![
            panel_title("Save as layout"),
            row![
                text_field("hotas-nov", &form.name, Message::SaveName)
                    .on_submit_maybe(ready.then_some(Message::SaveLayout(false)))
                    .width(layout::CONTROL_WIDTH),
                button(
                    "Save layout",
                    ButtonKind::Primary,
                    ready.then_some(Message::SaveLayout(false)),
                ),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        ]
        .spacing(space::MD);
        if !name.is_empty() && file.is_err() {
            content = content.push(error_text("Use only letters, digits, - and _.".to_string()));
        }
        content.push(hint(caption)).into()
    }

    fn failed<'a>(&'a self, failure: &'a Failure) -> Element<'a, Message> {
        let kind = if failure.unrecognised {
            ToastKind::Warning
        } else {
            ToastKind::Error
        };
        let mut body = Column::new().spacing(space::LG).height(Length::Fill);
        if self.sources.len() > 1 {
            body = body.push(self.source_pick());
        }
        body = body.push(banner(kind, &failure.banner, None));
        if !failure.raw.is_empty() {
            let lines = &failure.raw;
            body = body.push(
                container(virtual_list(
                    &self.raw_list,
                    lines.len(),
                    layout::DIFF_ROW_HEIGHT,
                    Message::RawScrolled,
                    move |index| raw_line(&lines[index]),
                ))
                .padding(space::XS)
                .style(theme::sunken)
                .height(Length::Fill),
            );
        }
        body.into()
    }
}

fn vertical_scroll<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(content)
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(theme::SCROLLBAR_WIDTH)
                .scroller_width(theme::SCROLLBAR_WIDTH)
                .margin(space::XS / 2.0),
        ))
        .style(theme::scroll)
        .height(Length::Fill)
        .into()
}

fn map_item<'a>(
    name: &'a str,
    count: usize,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    widget::button(
        row![
            label(name)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
            hint(count.to_string()),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .padding(LIST_ITEM_PADDING)
    .width(Length::Fill)
    .style(theme::row_button(selected))
    .on_press(message)
    .into()
}

fn header_row(model: &Model) -> Element<'_, Message> {
    let mut cells = vec![header_cell("Action", action_width())];
    for title in &model.column_labels {
        cells.push(header_cell(title, device_width()));
    }
    container(
        Row::with_children(cells)
            .spacing(CELL_GAP)
            .align_y(Alignment::Center),
    )
    .padding(ROW_SIDES)
    .height(layout::TABLE_ROW_HEIGHT)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .style(theme::table_header)
    .into()
}

fn header_cell(title: &str, width: Length) -> Element<'_, Message> {
    container(
        text(title)
            .size(size::CAPTION)
            .font(FONT_SEMIBOLD)
            .style(theme::muted)
            .wrapping(text::Wrapping::None),
    )
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        ..CELL_PADDING
    })
    .width(width)
    .clip(true)
    .into()
}

fn group_line(entry: &MapEntry) -> Element<'_, Message> {
    container(
        row![
            heading(entry.label.to_uppercase()),
            hint(binds_text(entry.count)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 0.0,
        right: space::LG,
        bottom: space::SM,
        left: space::LG,
    })
    .height(Length::Fill)
    .width(Length::Fill)
    .align_y(Alignment::End)
    .into()
}

fn caps_row(caps: &[String]) -> Element<'_, Message> {
    let mut parts = Row::new().spacing(space::XS).align_y(Alignment::Center);
    for (index, cap) in caps.iter().enumerate() {
        if index > 0 {
            parts = parts.push(hint("+"));
        }
        parts = parts.push(key_cap(cap.as_str()));
    }
    parts.into()
}

fn cell_content(cell: &Cell) -> Element<'_, Message> {
    let mut first = Row::new().spacing(space::XS).align_y(Alignment::Center);
    if cell.changed() {
        first = first.push(change_marker(true));
    }
    first = if cell.current.is_some() {
        first.push(caps_row(&cell.caps))
    } else {
        first.push(text("-").size(size::BODY).style(theme::muted))
    };
    if cell.conflict {
        first = first.push(icon_toned(icons::WARNING, layout::ICON, Tone::Warning));
    }
    let content: Element<'_, Message> = if cell.changed() {
        column![
            first,
            hint(cell.was.as_str()).wrapping(text::Wrapping::None)
        ]
        .spacing(TIGHT)
        .into()
    } else {
        first.into()
    };
    container(content)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .clip(true)
        .into()
}

fn panel_title(title: &str) -> Element<'_, Message> {
    row![
        text(title)
            .size(size::LABEL)
            .font(FONT_SEMIBOLD)
            .width(Length::Fill),
        icon_only(icons::CLOSE, Some(Message::ClosePanel)),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn labelled<'a>(title: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![heading(title), control.into()]
        .spacing(space::XS)
        .width(Length::Fill)
        .into()
}

fn error_text<'a>(error: String) -> Element<'a, Message> {
    text(error)
        .size(size::CAPTION)
        .style(theme::toned(Tone::Danger))
        .into()
}

fn binds_text(count: usize) -> String {
    if count == 1 {
        "1 bind".to_string()
    } else {
        format!("{count} binds")
    }
}

fn raw_line(line: &str) -> Element<'_, Message> {
    container(
        text(line)
            .font(FONT_MONO)
            .size(size::CAPTION)
            .wrapping(text::Wrapping::None),
    )
    .padding(Padding {
        top: 0.0,
        right: space::SM,
        bottom: 0.0,
        left: space::SM,
    })
    .height(Length::Fill)
    .align_y(Alignment::Center)
    .into()
}

fn edit_panel<'a>(model: &'a Model, edit: &'a Edit) -> Element<'a, Message> {
    let cell = model.cell(&edit.cell);
    let column_label = model
        .column(edit.cell.slot)
        .map_or("", |index| model.column_labels[index].as_str());
    let action_label = model
        .row(&edit.cell.map, &edit.cell.action)
        .map_or(edit.cell.action.as_str(), |row| row.label.as_str());
    let title = row![
        column![
            row![
                text(action_label).size(size::LABEL).font(FONT_SEMIBOLD),
                tag(column_label, Tone::Neutral),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
            hint(format!(
                "{} in {}",
                edit.cell.action,
                model.map_label(&edit.cell.map)
            )),
        ]
        .spacing(space::XS)
        .width(Length::Fill),
        current_bind(cell),
        icon_only(icons::CLOSE, Some(Message::ClosePanel)),
    ]
    .spacing(space::LG)
    .align_y(Alignment::Center);
    let main = if captures(edit.cell.slot.kind) {
        capture_box(edit)
    } else {
        quick_picks()
    };
    column![
        title,
        row![
            container(main).width(Length::Fill),
            container(input_side(edit, cell)).width(binds::EDIT_SIDE_WIDTH),
        ]
        .spacing(space::XL),
    ]
    .spacing(space::MD)
    .into()
}

/// The cell's input now and, once edited, what it was.
fn current_bind(cell: Option<&Cell>) -> Element<'_, Message> {
    let Some(cell) = cell else {
        return Space::new().into();
    };
    let now: Element<'_, Message> = if cell.current.is_some() {
        caps_row(&cell.caps)
    } else {
        muted("Not bound").into()
    };
    let mut content = column![
        row![hint("Now"), now]
            .spacing(space::SM)
            .align_y(Alignment::Center)
    ]
    .spacing(TIGHT)
    .align_x(Alignment::End);
    if cell.changed() {
        content = content.push(hint(cell.was.as_str()));
    }
    content.into()
}

fn capture_box<'a>(edit: &'a Edit) -> Element<'a, Message> {
    let body: Element<'a, Message> = if edit.listening {
        column![
            text(CAPTURE_PROMPT).size(size::LABEL).font(FONT_SEMIBOLD),
            hint("Hold Alt, Ctrl or Shift for a combination. Escape stops listening."),
        ]
        .spacing(space::XS)
        .align_x(Alignment::Center)
        .into()
    } else {
        row![
            muted("Not listening"),
            button("Listen again", ButtonKind::Secondary, Some(Message::Listen)),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center)
        .into()
    };
    let area = mouse_area(
        container(body)
            .center_x(Length::Fill)
            .center_y(binds::CAPTURE_HEIGHT)
            .style(theme::capture_box(edit.listening)),
    )
    .on_enter(Message::CaptureHover(true))
    .on_exit(Message::CaptureHover(false));
    let mut content = column![area].spacing(space::SM);
    if let Some(note) = edit.note {
        content = content.push(
            text(note)
                .size(size::CAPTION)
                .style(theme::toned(Tone::Warning)),
        );
    }
    content.into()
}

fn chip<'a>(name: String, fixed: bool) -> Element<'a, Message> {
    let caption = text(quick_label(&name)).size(size::CAPTION);
    let face = if fixed {
        caption.width(Length::Fill).align_x(Alignment::Center)
    } else {
        caption
    };
    let chip = widget::button(face)
        .padding(CHIP_PADDING)
        .style(theme::secondary)
        .on_press(Message::QuickPick(name));
    if fixed {
        chip.width(binds::QUICK_PICK_WIDTH).into()
    } else {
        chip.into()
    }
}

fn quick_picks<'a>() -> Element<'a, Message> {
    let buttons = Row::with_children(quick_buttons().into_iter().map(|name| chip(name, true)))
        .spacing(space::XS)
        .wrap()
        .vertical_spacing(space::XS);
    let others = AXES
        .iter()
        .map(|axis| axis.to_string())
        .chain(quick_hats())
        .map(|name| chip(name, false));
    let others = Row::with_children(others)
        .spacing(space::XS)
        .wrap()
        .vertical_spacing(space::XS);
    column![
        heading("Buttons"),
        buttons,
        heading("Axes and hat 1"),
        others
    ]
    .spacing(space::SM)
    .into()
}

fn placeholder(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Keyboard => "lalt+f",
        DeviceKind::Mouse => "mouse1",
        DeviceKind::Joystick => "button5",
        DeviceKind::Gamepad => "shoulderl",
    }
}

fn input_side<'a>(edit: &'a Edit, cell: Option<&'a Cell>) -> Element<'a, Message> {
    let judged = judge(edit.cell.slot, &edit.typed);
    let valid = matches!(judged, Typed::Valid(_));
    let field = text_field(
        placeholder(edit.cell.slot.kind),
        &edit.typed,
        Message::Typed,
    )
    .on_submit_maybe(valid.then_some(Message::Set))
    .width(Length::Fill);
    let mut content = column![
        heading("Input as the game writes it"),
        row![
            field,
            button("Set", ButtonKind::Primary, valid.then_some(Message::Set))
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    ]
    .spacing(space::SM);
    if let Typed::Invalid(error) = judged {
        content = content.push(error_text(error));
    }
    let bound = cell.is_some_and(|cell| cell.current.is_some());
    content
        .push(
            row![
                column![
                    button(
                        "Reset to default",
                        ButtonKind::Secondary,
                        bound.then_some(Message::Reset)
                    ),
                    hint(RESET_CAPTION),
                ]
                .spacing(space::XS)
                .width(Length::Fill),
                column![
                    button("Unbind", ButtonKind::Ghost, None),
                    hint(UNBIND_CAPTION)
                ]
                .spacing(space::XS)
                .width(Length::Fill),
            ]
            .spacing(space::MD),
        )
        .into()
}
