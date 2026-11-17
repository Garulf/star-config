use super::inputs::{
    AXES, Known, Typed, captures, judge, kind_label, quick_buttons, quick_hats, quick_label,
    slot_label, stick_label,
};
use super::model::{Cell, CellRef, Line, MapEntry, Model, Row as BindRow};
use super::source::{Failure, UNRECOGNISED};
use super::{
    ADD_CAPTION, AddForm, CAPTURE_PROMPT, Doc, EMPTY_BODY, EMPTY_TITLE, Edit, GAME_DATA_LOADING,
    Message, NOT_IN_BUILD, RESET_CAPTION, SHOW_UNLISTED, SaveForm, State, SwapForm, TRY_AGAIN, Tab,
    UNBIND_CAPTION, add_input,
};
use crate::icons;
use crate::screen::{Screen, ToastKind};
use crate::theme::{self, ButtonKind, FONT_MONO, FONT_SEMIBOLD, Tone, binds, layout, size, space};
use crate::widgets::{
    banner, button, change_marker, checkbox, divider, empty_state, heading, hint, icon_only,
    icon_toned, key_cap, label, muted, page_fixed, pick_list, search_field, table_header, tag,
    text_field, virtual_list, virtual_list_with_id,
};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    self, Column, Id, Row, Space, column, container, mouse_area, row, scrollable, sensor, text,
};
use iced::{Alignment, Element, Length, Padding};
use sc_core::actionmaps::{DeviceKind, layout_file_name};
use sc_core::binds::{KINDS, Origin};

pub(super) const ROW_HEIGHT: f32 = layout::ROW_MIN_HEIGHT;
pub(super) const TABLE_ID: &str = "keybinds-table";
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
const UNBOUND: &str = "Unbound";

fn action_width() -> Length {
    Length::FillPortion(binds::ACTION_PORTION)
}

/// The game's mouse names and the stick's name need more room than keys and pad buttons.
fn device_width(kind: DeviceKind) -> Length {
    match kind {
        DeviceKind::Mouse | DeviceKind::Joystick => Length::FillPortion(binds::WIDE_DEVICE_PORTION),
        DeviceKind::Keyboard | DeviceKind::Gamepad => Length::FillPortion(binds::DEVICE_PORTION),
    }
}

fn several_entries(kind: DeviceKind) -> String {
    format!(
        "Your file has more than one {} entry for this action. Set, Unbind and Reset replace them all.",
        kind_label(kind).to_lowercase()
    )
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
            (Doc::Ready { .. } | Doc::Away, Some(model)) => match self.tab {
                Tab::Binds => self.main(model),
                Tab::Advanced => self.advanced_page(model),
            },
            _ => {
                let note = if self.game_loading {
                    GAME_DATA_LOADING
                } else {
                    "Reading your keybinds..."
                };
                page_fixed(
                    title,
                    None,
                    Vec::new(),
                    container(hint(note)).center(Length::Fill),
                )
            }
        }
    }

    fn main<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        let mut body = column![self.toolbar(model)]
            .spacing(space::LG)
            .height(Length::Fill);
        if !self.writable() && matches!(self.doc, Doc::Ready { .. }) {
            body = body.push(banner(ToastKind::Warning, UNRECOGNISED, None));
        }
        if let Some(note) = self.game_note() {
            body = body.push(note);
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

    /// Why the game's defaults are missing, with a retry, or that they are still being read.
    pub(super) fn game_note(&self) -> Option<Element<'_, Message>> {
        if let Some(note) = &self.no_game_note {
            return Some(banner(
                ToastKind::Info,
                note,
                Some((TRY_AGAIN, Message::RetryGameData)),
            ));
        }
        self.game_loading
            .then(|| banner(ToastKind::Info, GAME_DATA_LOADING, None))
    }

    pub(super) fn source_pick(&self) -> Element<'_, Message> {
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
        let mut bar = row![self.tabs(), self.source_pick()]
            .spacing(space::SM)
            .align_y(Alignment::Center);
        if self.source.as_ref().is_some_and(super::Source::is_layout) {
            bar = bar.push(button(
                "Save as layout...",
                ButtonKind::Secondary,
                self.can_save_layout().then_some(Message::OpenSaveAs),
            ));
        }
        bar = bar.push(Space::new().width(Length::Fill));
        if self.game().is_some() {
            bar = bar.push(checkbox(
                SHOW_UNLISTED,
                self.show_unlisted,
                Message::ShowUnlisted,
            ));
        }
        bar = bar.push(self.conflicts_chip(model.conflicts));
        if model.joysticks().len() >= 2 {
            bar = bar.push(button(
                "Swap joysticks",
                ButtonKind::Secondary,
                writable.then_some(Message::OpenSwap),
            ));
        }
        if self.game().is_none() && !self.game_loading {
            bar = bar.push(button(
                "Add a bind by name",
                ButtonKind::Secondary,
                writable.then_some(Message::OpenAdd),
            ));
        }
        bar.into()
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
            model.total(self.show_unlisted),
            self.map.is_none(),
            Message::PickMap(None),
        ));
        let mut category: Option<&str> = None;
        for entry in &model.maps {
            let count = entry.count(self.show_unlisted);
            if count == 0 {
                continue;
            }
            if let Some(name) = entry.category.as_deref()
                && category != Some(name)
            {
                items = items.push(category_heading(name));
                category = Some(name);
            }
            items = items.push(map_item(
                &entry.label,
                count,
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
            virtual_list_with_id(
                Id::new(TABLE_ID),
                &self.list,
                self.lines.len(),
                ROW_HEIGHT,
                Message::Scrolled,
                move |index| self.line(model, self.lines[index]),
            )
        };
        column![
            header_row(),
            sensor(container(table).height(Length::Fill))
                .on_show(|size| Message::TableResized(size.height))
                .on_resize(|size| Message::TableResized(size.height)),
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
            Line::Group(index) => group_line(&model.maps[index], self.show_unlisted),
            Line::Action(index) => self.action_row(model, &model.rows[index]),
        }
    }

    fn action_row<'a>(&'a self, model: &'a Model, bind: &'a BindRow) -> Element<'a, Message> {
        let mut name = row![label(bind.label.as_str()).wrapping(text::Wrapping::None)]
            .spacing(space::SM)
            .align_y(Alignment::Center);
        if !bind.in_build {
            name = name.push(tag(NOT_IN_BUILD, Tone::Warning));
        }
        let mut cells: Vec<Element<'a, Message>> = vec![
            container(
                column![
                    name,
                    hint(bind.action.as_str()).wrapping(text::Wrapping::None),
                ]
                .spacing(TIGHT),
            )
            .padding(CELL_PADDING)
            .width(action_width())
            .clip(true)
            .into(),
        ];
        for cell in &bind.cells {
            cells.push(self.bind_cell(model, bind, cell));
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
        model: &'a Model,
        bind: &'a BindRow,
        cell: &'a Cell,
    ) -> Element<'a, Message> {
        let target = CellRef {
            map: bind.map.clone(),
            action: bind.action.clone(),
            kind: cell.kind,
        };
        let selected = matches!(&self.panel, super::Panel::Edit(edit) if edit.cell == target);
        widget::button(cell_content(model, cell))
            .padding(CELL_PADDING)
            .width(device_width(cell.kind))
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
            super::Panel::Edit(edit) => self.edit_panel(model, edit),
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
            .map(|choice| judge(choice.slot, &form.input, Known::default()));
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

fn header_row() -> Element<'static, Message> {
    let mut cells = vec![header_cell("Action", action_width())];
    for kind in KINDS {
        cells.push(header_cell(kind_label(kind), device_width(kind)));
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

fn category_heading(name: &str) -> Element<'_, Message> {
    container(heading(name.to_uppercase()))
        .padding(Padding {
            top: space::MD,
            bottom: space::XS,
            ..LIST_ITEM_PADDING
        })
        .into()
}

fn group_line(entry: &MapEntry, show_unlisted: bool) -> Element<'_, Message> {
    container(
        row![
            heading(entry.label.to_uppercase()),
            hint(actions_text(entry.count(show_unlisted))),
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

/// The bind with its marker, the stick for a joystick, and a second line naming the stick's
/// product or, after a staged edit, what the cell was.
fn cell_content<'a>(model: &'a Model, cell: &'a Cell) -> Element<'a, Message> {
    let mut first = Row::new().spacing(space::XS).align_y(Alignment::Center);
    if cell.changed {
        first = first.push(change_marker(true));
    }
    first = first.push(bind_face(cell));
    let stick = cell.slot().filter(|slot| slot.kind == DeviceKind::Joystick);
    if let Some(slot) = stick {
        first = first.push(hint(stick_label(slot)));
    }
    if cell.conflict {
        first = first.push(icon_toned(icons::WARNING, layout::ICON, Tone::Warning));
    }
    let second = cell.was.as_deref().or_else(|| {
        stick
            .and_then(|slot| model.products.get(&slot))
            .map(String::as_str)
    });
    let content: Element<'_, Message> = match second {
        Some(second) => column![first, hint(second).wrapping(text::Wrapping::None)]
            .spacing(TIGHT)
            .into(),
        None => first.into(),
    };
    container(content)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .clip(true)
        .into()
}

fn bind_face(cell: &Cell) -> Element<'_, Message> {
    match (&cell.input, cell.origin) {
        (Some(_), _) => caps_row(&cell.caps),
        (None, Origin::Unbound) => hint(UNBOUND).into(),
        (None, _) => text("-").size(size::BODY).style(theme::muted).into(),
    }
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

fn actions_text(count: usize) -> String {
    if count == 1 {
        "1 action".to_string()
    } else {
        format!("{count} actions")
    }
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

impl State {
    fn edit_panel<'a>(&'a self, model: &'a Model, edit: &'a Edit) -> Element<'a, Message> {
        let cell = model.cell(&edit.cell);
        let bind = model.row(&edit.cell.map, &edit.cell.action);
        let action_label = bind.map_or(edit.cell.action.as_str(), |row| row.label.as_str());
        let mut about = column![
            row![
                text(action_label).size(size::LABEL).font(FONT_SEMIBOLD),
                tag(kind_label(edit.cell.kind), Tone::Neutral),
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
        .width(Length::Fill);
        if let Some(description) = bind.and_then(|row| row.description.as_deref()) {
            about = about.push(hint(description));
        }
        let title = row![
            about,
            current_bind(cell, self.game().is_some()),
            icon_only(icons::CLOSE, Some(Message::ClosePanel)),
        ]
        .spacing(space::LG)
        .align_y(Alignment::Center);
        let main = if captures(edit.cell.kind) {
            capture_box(edit)
        } else {
            quick_picks()
        };
        column![
            title,
            row![
                container(main).width(Length::Fill),
                container(self.input_side(edit, cell)).width(binds::EDIT_SIDE_WIDTH),
            ]
            .spacing(space::XL),
        ]
        .spacing(space::MD)
        .into()
    }

    fn input_side<'a>(&'a self, edit: &'a Edit, cell: Option<&'a Cell>) -> Element<'a, Message> {
        let judged = judge(edit.slot, &edit.typed, self.known(&edit.cell));
        let valid = matches!(judged, Typed::Valid(_));
        let field = text_field(placeholder(edit.cell.kind), &edit.typed, Message::Typed)
            .on_submit_maybe(valid.then_some(Message::Set))
            .width(Length::Fill);
        let mut content = Column::new().spacing(space::SM);
        if edit.cell.kind == DeviceKind::Joystick {
            let stick = self.slot_choice(edit.slot);
            content = content
                .push(heading("Stick and input as the game writes it"))
                .push(
                    pick_list(self.stick_choices(), Some(stick), Message::PickStick)
                        .width(Length::Fill),
                );
        } else {
            content = content.push(heading("Input as the game writes it"));
        }
        content = content.push(
            row![
                field,
                button("Set", ButtonKind::Primary, valid.then_some(Message::Set))
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );
        if let Typed::Invalid(error) = judged {
            content = content.push(error_text(error));
        }
        let Some(cell) = cell else {
            return content.into();
        };
        content = content.push(cell_actions(cell));
        if cell.extra > 0 {
            content = content.push(hint(several_entries(cell.kind)));
        }
        content.into()
    }
}

fn cell_actions(cell: &Cell) -> Element<'_, Message> {
    row![
        column![
            button(
                "Unbind",
                ButtonKind::Secondary,
                cell.unbindable().then_some(Message::Unbind)
            ),
            hint(UNBIND_CAPTION),
        ]
        .spacing(space::XS)
        .width(Length::Fill),
        column![
            button(
                "Reset to default",
                ButtonKind::Secondary,
                cell.resettable().then_some(Message::Reset)
            ),
            hint(RESET_CAPTION),
        ]
        .spacing(space::XS)
        .width(Length::Fill),
    ]
    .spacing(space::MD)
    .into()
}

fn game_default(cell: &Cell) -> Element<'_, Message> {
    let value: Element<'_, Message> = if cell.default.is_some() {
        caps_row(&cell.default_caps)
    } else {
        muted("None").into()
    };
    row![hint("Game default"), value]
        .spacing(space::SM)
        .align_y(Alignment::Center)
        .into()
}

/// The cell's input now, the game's default a reset returns to, and once edited what it was.
fn current_bind(cell: Option<&Cell>, with_default: bool) -> Element<'_, Message> {
    let Some(cell) = cell else {
        return Space::new().into();
    };
    let now: Element<'_, Message> = match (&cell.input, cell.origin) {
        (Some(_), _) => caps_row(&cell.caps),
        (None, Origin::Unbound) => muted(UNBOUND).into(),
        (None, _) => muted("Not bound").into(),
    };
    let mut content = column![
        row![hint("Now"), now]
            .spacing(space::SM)
            .align_y(Alignment::Center)
    ]
    .spacing(TIGHT)
    .align_x(Alignment::End);
    if with_default {
        content = content.push(game_default(cell));
    }
    if let Some(was) = &cell.was {
        content = content.push(hint(was.as_str()));
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
