//! The Advanced controls view: the game's option groups per device slot, with their invert,
//! exponent, sensitivity and response curve, and each device's axis values.

mod sheet;

pub use sheet::DevicePick;

use super::model::Model;
use super::source::UNRECOGNISED;
use super::{Doc, Message, NOT_IN_BUILD, State, Tab, game_of};
use crate::icons;
use crate::screen::{Action, Screen, ToastKind};
use crate::theme::advanced as metrics;
use crate::theme::{self, ButtonKind, FONT_SEMIBOLD, Tone, binds, layout, size, space};
use crate::widgets::{
    banner, button, cell, change_marker, heading, hint, icon_only, label, number_field, page_fixed,
    rows, section, segmented, slider, strong, table_header, table_row, tag, text_field, toggle,
};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{self, Column, Row, Space, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};
use sc_core::actionmaps::{AxisField, GroupField, Op};
use sheet::{
    AXIS_FIELDS, AxisRow, DeviceEntry, Files, GroupLine, Points, Setting, Sheet, in_order,
    new_point, number, parse_points, samples,
};

pub const BINDS_TAB: &str = "Binds";
pub const ADVANCED_TAB: &str = "Advanced controls";
pub const NOT_CONNECTED: &str = "Not connected to a joystick slot";
pub const GAME_DEFAULT: &str = "Game default";
pub const NOT_A_NUMBER: &str = "Type a number, for example 1.5.";
pub const POINT_RANGE: &str = "Each point needs an in and an out value from 0 to 1.";
const NO_GROUPS: &str = "Your file changes no options for this device.";
const NO_AXES: &str = "Your file sets no deadzone, saturation or acceleration for this device.";
const NO_POINTS: &str = "No points yet. Add one to shape the response.";
const DEFAULT_VALUE: &str = "Default";
const PLOT_BARS: usize = 24;
const SLIDER_STEP: f64 = 0.01;
/// Where an unset sensitivity slider rests: the game's unscaled response.
const SENSITIVITY_REST: f64 = 1.0;

/// A value the view can change.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionField {
    Group {
        group: String,
        field: GroupField,
    },
    Curve {
        group: String,
    },
    Axis {
        product: String,
        input: String,
        field: AxisField,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointPart {
    In,
    Out,
}

#[derive(Debug, Clone)]
pub enum AdvancedMessage {
    Pick(DevicePick),
    Invert(String, bool),
    Typed(OptionField, String),
    Submit(OptionField),
    Slide(OptionField, f64),
    Release,
    Reset(OptionField),
    OpenCurve(String),
    CloseCurve,
    PointTyped(usize, PointPart, String),
    SubmitPoints,
    AddPoint,
    RemovePoint(usize),
}

struct CurveEdit {
    group: String,
    points: Points,
    error: Option<&'static str>,
}

pub(super) struct Advanced {
    pick: DevicePick,
    sheet: Option<Sheet>,
    drafts: Vec<(OptionField, String)>,
    sliding: Option<(OptionField, f64)>,
    error: Option<(OptionField, &'static str)>,
    curve: Option<CurveEdit>,
}

impl Advanced {
    pub(super) fn new() -> Self {
        Self {
            pick: DevicePick::KeyboardMouse,
            sheet: None,
            drafts: Vec::new(),
            sliding: None,
            error: None,
            curve: None,
        }
    }

    fn rebuild(&mut self, game: Option<&sc_core::defaults::GameData>, files: Files<'_>) {
        let mut sheet = Sheet::build(game, files, &self.pick);
        if sheet.entry(&self.pick).is_none() {
            self.pick = DevicePick::KeyboardMouse;
            sheet = Sheet::build(game, files, &self.pick);
        }
        self.drafts.clear();
        self.sliding = None;
        self.error = None;
        if let Some(edit) = &mut self.curve {
            match sheet
                .group(&edit.group)
                .and_then(|line| line.curve.as_ref())
            {
                Some(curve) => {
                    edit.points = curve.effective().cloned().unwrap_or_default();
                    edit.error = None;
                }
                None => self.curve = None,
            }
        }
        self.sheet = Some(sheet);
    }

    fn clear(&mut self) {
        let pick = self.pick.clone();
        *self = Self::new();
        self.pick = pick;
    }

    fn draft(&self, field: &OptionField) -> Option<&str> {
        self.drafts
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, text)| text.as_str())
    }

    fn sliding(&self, field: &OptionField) -> Option<f64> {
        self.sliding
            .as_ref()
            .filter(|(f, _)| f == field)
            .map(|(_, value)| *value)
    }

    fn error(&self, field: &OptionField) -> Option<&'static str> {
        self.error
            .as_ref()
            .filter(|(f, _)| f == field)
            .map(|(_, error)| *error)
    }

    fn set_draft(&mut self, field: OptionField, text: String) {
        if self.error.as_ref().is_some_and(|(f, _)| *f == field) {
            self.error = None;
        }
        match self.drafts.iter_mut().find(|(f, _)| *f == field) {
            Some((_, draft)) => *draft = text,
            None => self.drafts.push((field, text)),
        }
    }

    /// The staged value of a field, when the file holds one.
    fn held(&self, field: &OptionField) -> bool {
        let Some(sheet) = &self.sheet else {
            return false;
        };
        match field {
            OptionField::Group { group, field } => {
                sheet.group(group).is_some_and(|line| match field {
                    GroupField::Invert => line.invert.as_ref().is_some_and(|s| s.now.is_some()),
                    GroupField::Exponent => line.exponent.as_ref().is_some_and(|s| s.now.is_some()),
                    GroupField::Sensitivity => {
                        line.sensitivity.as_ref().is_some_and(|s| s.now.is_some())
                    }
                })
            }
            OptionField::Curve { group } => sheet
                .group(group)
                .and_then(|line| line.curve.as_ref())
                .is_some_and(|s| s.now.is_some()),
            OptionField::Axis {
                product,
                input,
                field,
            } => sheet
                .axis(product, input, *field)
                .is_some_and(|s| s.now.is_some()),
        }
    }

    fn clamp(&self, field: &OptionField, value: f64) -> f64 {
        match field {
            OptionField::Group { .. } => match self.sheet.as_ref().and_then(|s| s.sensitivity) {
                Some((min, max)) => value.clamp(min, max),
                None => value,
            },
            _ => value.clamp(0.0, 1.0),
        }
    }

    fn set_op(&self, field: &OptionField, value: &str) -> Option<Op> {
        match field {
            OptionField::Group { group, field } => {
                let slot = self.pick.slot()?;
                Some(Op::SetGroup {
                    kind: slot.kind,
                    instance: slot.instance,
                    group: group.clone(),
                    field: *field,
                    value: value.to_string(),
                })
            }
            OptionField::Axis {
                product,
                input,
                field,
            } => Some(Op::SetAxis {
                product: product.clone(),
                input: input.clone(),
                field: *field,
                value: value.to_string(),
            }),
            OptionField::Curve { .. } => None,
        }
    }

    fn reset_op(&self, field: &OptionField) -> Option<Op> {
        if !self.held(field) {
            return None;
        }
        match field {
            OptionField::Group { group, field } => {
                let slot = self.pick.slot()?;
                Some(Op::ResetGroup {
                    kind: slot.kind,
                    instance: slot.instance,
                    group: group.clone(),
                    field: *field,
                })
            }
            OptionField::Curve { group } => {
                let slot = self.pick.slot()?;
                Some(Op::ResetCurve {
                    kind: slot.kind,
                    instance: slot.instance,
                    group: group.clone(),
                })
            }
            OptionField::Axis {
                product,
                input,
                field,
            } => Some(Op::ResetAxis {
                product: product.clone(),
                input: input.clone(),
                field: *field,
            }),
        }
    }

    /// Stages the curve being edited with these points, sorted by input.
    fn curve_op(&mut self, points: Points) -> Option<Op> {
        let slot = self.pick.slot()?;
        let edit = self.curve.as_mut()?;
        let Some(points) = in_order(&points) else {
            edit.error = Some(POINT_RANGE);
            return None;
        };
        Some(Op::SetCurve {
            kind: slot.kind,
            instance: slot.instance,
            group: edit.group.clone(),
            points,
        })
    }
}

fn wrap(message: AdvancedMessage) -> Message {
    Message::Advanced(message)
}

impl State {
    pub(super) fn show_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Advanced {
            self.panel = super::Panel::Closed;
        }
    }

    /// Rebuilds the view after the document or the game's defaults changed. While a write
    /// is away the last sheet stays on screen.
    pub(super) fn rebuild_advanced(&mut self) {
        match &self.doc {
            Doc::Ready { staged, .. } => {
                let files = Files {
                    now: staged.current(),
                    was: staged.base(),
                };
                self.advanced.rebuild(game_of(&self.game), files);
            }
            Doc::Away => {}
            _ => self.advanced.clear(),
        }
    }

    pub(super) fn advanced_update(&mut self, message: AdvancedMessage) -> Action<Message> {
        let op = match message {
            AdvancedMessage::Pick(pick) => {
                self.advanced.pick = pick;
                self.advanced.curve = None;
                self.rebuild_advanced();
                None
            }
            AdvancedMessage::Invert(group, on) => self.advanced.set_op(
                &OptionField::Group {
                    group,
                    field: GroupField::Invert,
                },
                if on { "1" } else { "0" },
            ),
            AdvancedMessage::Typed(field, typed) => {
                self.advanced.set_draft(field, typed);
                None
            }
            AdvancedMessage::Submit(field) => self.submit(&field),
            AdvancedMessage::Slide(field, value) => {
                let value = self.advanced.clamp(&field, value);
                self.advanced.sliding = Some((field, value));
                None
            }
            AdvancedMessage::Release => self
                .advanced
                .sliding
                .take()
                .and_then(|(field, value)| self.advanced.set_op(&field, &number(value))),
            AdvancedMessage::Reset(field) => self.advanced.reset_op(&field),
            AdvancedMessage::OpenCurve(group) => {
                self.open_curve(group);
                None
            }
            AdvancedMessage::CloseCurve => {
                self.advanced.curve = None;
                None
            }
            AdvancedMessage::PointTyped(index, part, typed) => {
                self.type_point(index, part, typed);
                None
            }
            AdvancedMessage::SubmitPoints => {
                let points = self.advanced.curve.as_ref().map(|edit| edit.points.clone());
                points.and_then(|points| self.advanced.curve_op(points))
            }
            AdvancedMessage::AddPoint => self.add_point(),
            AdvancedMessage::RemovePoint(index) => self.remove_point(index),
        };
        match op {
            Some(op) => self.stage(op),
            None => Action::none(),
        }
    }

    fn submit(&mut self, field: &OptionField) -> Option<Op> {
        let typed = self.advanced.draft(field)?.trim().to_string();
        if !typed.parse::<f64>().is_ok_and(f64::is_finite) {
            self.advanced.error = Some((field.clone(), NOT_A_NUMBER));
            return None;
        }
        self.advanced.set_op(field, &typed)
    }

    fn open_curve(&mut self, group: String) {
        let Some(curve) = self
            .advanced
            .sheet
            .as_ref()
            .and_then(|sheet| sheet.group(&group))
            .and_then(|line| line.curve.as_ref())
        else {
            return;
        };
        let points = curve.effective().cloned().unwrap_or_default();
        self.advanced.curve = Some(CurveEdit {
            group,
            points,
            error: None,
        });
    }

    fn type_point(&mut self, index: usize, part: PointPart, typed: String) {
        let Some(edit) = &mut self.advanced.curve else {
            return;
        };
        let Some(point) = edit.points.get_mut(index) else {
            return;
        };
        match part {
            PointPart::In => point.0 = typed,
            PointPart::Out => point.1 = typed,
        }
        edit.error = None;
    }

    fn add_point(&mut self) -> Option<Op> {
        let edit = self.advanced.curve.as_mut()?;
        let Some(numbers) = parse_points(&edit.points) else {
            edit.error = Some(POINT_RANGE);
            return None;
        };
        let (x, y) = new_point(&numbers);
        let mut points = edit.points.clone();
        points.push((number(x), number(y)));
        self.advanced.curve_op(points)
    }

    /// Removing the last point of the file's curve resets it to the game's.
    fn remove_point(&mut self, index: usize) -> Option<Op> {
        let edit = self.advanced.curve.as_mut()?;
        if index >= edit.points.len() {
            return None;
        }
        let mut points = edit.points.clone();
        points.remove(index);
        if !points.is_empty() {
            return self.advanced.curve_op(points);
        }
        let group = edit.group.clone();
        edit.points.clear();
        self.advanced.reset_op(&OptionField::Curve { group })
    }

    pub(super) fn tabs(&self) -> Element<'_, Message> {
        segmented(vec![
            (
                BINDS_TAB,
                self.tab == Tab::Binds,
                Message::ShowTab(Tab::Binds),
            ),
            (
                ADVANCED_TAB,
                self.tab == Tab::Advanced,
                Message::ShowTab(Tab::Advanced),
            ),
        ])
    }

    pub(super) fn advanced_page<'a>(&'a self, model: &'a Model) -> Element<'a, Message> {
        let toolbar = row![self.tabs(), self.source_pick()]
            .spacing(space::SM)
            .align_y(Alignment::Center);
        let mut body = column![toolbar].spacing(space::LG).height(Length::Fill);
        if !self.writable() && matches!(self.doc, Doc::Ready { .. }) {
            body = body.push(banner(ToastKind::Warning, UNRECOGNISED, None));
        }
        if let Some(note) = &self.no_game_note {
            body = body.push(banner(ToastKind::Info, note, None));
        }
        if let Some(sheet) = &self.advanced.sheet {
            body = body.push(
                row![
                    container(device_list(sheet, &self.advanced.pick))
                        .width(binds::MAP_LIST_WIDTH)
                        .height(Length::Fill)
                        .style(theme::card),
                    container(self.device_detail(sheet))
                        .width(Length::Fill)
                        .height(Length::Fill),
                ]
                .spacing(space::LG)
                .height(Length::Fill),
            );
        }
        page_fixed(
            Screen::Keybinds.label(),
            Some(model.devices.as_str()),
            Vec::new(),
            body,
        )
    }

    fn device_detail<'a>(&'a self, sheet: &'a Sheet) -> Element<'a, Message> {
        let pick = &self.advanced.pick;
        let mut header = column![text(pick.title()).size(size::LABEL).font(FONT_SEMIBOLD)];
        let subtitle = match pick {
            DevicePick::Unmapped(_) => Some(NOT_CONNECTED),
            _ => sheet.entry(pick).and_then(|entry| entry.product.as_deref()),
        };
        if let Some(subtitle) = subtitle {
            header = header.push(hint(subtitle));
        }
        let mut content = column![header].spacing(space::XL);
        if pick.slot().is_some() {
            content = content.push(section("Options", self.group_rows(sheet)));
        }
        content = content.push(self.axes(sheet));
        vertical_scroll(content.padding(Padding {
            right: space::LG,
            bottom: space::LG,
            ..Padding::ZERO
        }))
    }

    fn group_rows<'a>(&'a self, sheet: &'a Sheet) -> Element<'a, Message> {
        if sheet.groups.is_empty() {
            return container(hint(NO_GROUPS)).padding(space::LG).into();
        }
        rows(
            sheet
                .groups
                .iter()
                .map(|line| self.group_block(sheet, line)),
        )
    }

    fn group_block<'a>(&'a self, sheet: &'a Sheet, line: &'a GroupLine) -> Element<'a, Message> {
        if !line.has_controls() {
            return container(heading(line.label.as_str()))
                .padding(indented(line.depth, space::SM))
                .into();
        }
        let mut title = row![strong(line.label.as_str())]
            .spacing(space::SM)
            .align_y(Alignment::Center);
        if !line.in_build {
            title = title.push(tag(NOT_IN_BUILD, Tone::Warning));
        }
        let mut block = Column::new().push(container(title).padding(Padding {
            bottom: 0.0,
            ..indented(line.depth, space::MD)
        }));
        let controls = container(self.controls(sheet, line)).padding(Padding {
            left: control_indent(line.depth),
            ..Padding::ZERO
        });
        block = block.push(controls);
        block.into()
    }

    fn controls<'a>(&'a self, sheet: &'a Sheet, line: &'a GroupLine) -> Element<'a, Message> {
        let mut list = Column::new();
        let field = |field| OptionField::Group {
            group: line.name.clone(),
            field,
        };
        if let Some(invert) = &line.invert {
            list = list.push(self.invert_row(line, invert));
        }
        if let Some(sensitivity) = &line.sensitivity {
            let field = field(GroupField::Sensitivity);
            list = list.push(match sheet.sensitivity {
                Some(range) => self.sensitivity_row(field, sensitivity, range),
                None => self.number_row("Sensitivity", field, sensitivity),
            });
        }
        if let Some(exponent) = &line.exponent {
            list = list.push(self.number_row("Exponent", field(GroupField::Exponent), exponent));
        }
        if let Some(curve) = &line.curve {
            list = list.push(self.curve_row(line, curve));
        }
        list.into()
    }

    /// Reset, in a slot that keeps its width when there is nothing to reset.
    fn reset_button<'a>(&self, field: OptionField) -> Element<'a, Message> {
        let enabled = self.writable() && self.advanced.held(&field);
        let content: Element<'a, Message> = if enabled {
            button(
                "Reset",
                ButtonKind::Ghost,
                Some(wrap(AdvancedMessage::Reset(field))),
            )
        } else {
            Space::new().into()
        };
        container(content)
            .width(metrics::RESET_WIDTH)
            .align_x(Alignment::End)
            .into()
    }

    fn invert_row<'a>(
        &'a self,
        line: &'a GroupLine,
        setting: &'a Setting<bool>,
    ) -> Element<'a, Message> {
        let group = line.name.clone();
        let on = setting.effective().copied().unwrap_or(false);
        let mut control = Row::new().spacing(space::MD).align_y(Alignment::Center);
        if setting.effective().is_none() {
            control = control.push(text(GAME_DEFAULT).size(size::BODY).style(theme::muted));
        }
        let control = control
            .push(toggle(on, move |on| {
                wrap(AdvancedMessage::Invert(group.clone(), on))
            }))
            .push(self.reset_button(OptionField::Group {
                group: line.name.clone(),
                field: GroupField::Invert,
            }));
        control_row("Invert", notes(setting, on_off), setting.changed(), control)
    }

    fn number_row<'a>(
        &'a self,
        name: &'static str,
        field: OptionField,
        setting: &'a Setting<String>,
    ) -> Element<'a, Message> {
        let draft = self.advanced.draft(&field);
        let value = draft
            .or(setting.effective().map(String::as_str))
            .unwrap_or_default();
        let typed = field.clone();
        let input = text_field(GAME_DEFAULT, value, move |text| {
            wrap(AdvancedMessage::Typed(typed.clone(), text))
        })
        .on_submit(wrap(AdvancedMessage::Submit(field.clone())))
        .width(metrics::NUMBER_FIELD_WIDTH)
        .align_x(Alignment::End);
        let mut control = row![input].spacing(space::SM).align_y(Alignment::Center);
        if draft.is_some() {
            control = control.push(button(
                "Set",
                ButtonKind::Secondary,
                self.writable()
                    .then(|| wrap(AdvancedMessage::Submit(field.clone()))),
            ));
        }
        let mut lines = notes(setting, String::clone);
        lines.extend(self.advanced.error(&field).map(str::to_string));
        control_row(
            name,
            lines,
            setting.changed(),
            control.push(self.reset_button(field)),
        )
    }

    fn sensitivity_row<'a>(
        &'a self,
        field: OptionField,
        setting: &'a Setting<String>,
        (min, max): (f64, f64),
    ) -> Element<'a, Message> {
        let sliding = self.advanced.sliding(&field);
        let value = sliding
            .or_else(|| parse(setting.effective()))
            .unwrap_or(SENSITIVITY_REST)
            .clamp(min, max);
        let shown = sliding.map(number).or_else(|| setting.effective().cloned());
        let moved = field.clone();
        let control = row![
            slider(min..=max, value, move |value| {
                wrap(AdvancedMessage::Slide(moved.clone(), value))
            })
            .step(SLIDER_STEP)
            .on_release(wrap(AdvancedMessage::Release)),
            value_label(shown, setting.staged() || sliding.is_some()),
            self.reset_button(field),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        control_row(
            "Sensitivity",
            notes(setting, String::clone),
            setting.changed(),
            control,
        )
    }

    fn curve_row<'a>(
        &'a self,
        line: &'a GroupLine,
        setting: &'a Setting<Points>,
    ) -> Element<'a, Message> {
        let open = self
            .advanced
            .curve
            .as_ref()
            .filter(|edit| edit.group == line.name);
        let toggle_editor = match open {
            Some(_) => button(
                "Close",
                ButtonKind::Secondary,
                Some(wrap(AdvancedMessage::CloseCurve)),
            ),
            None => button(
                "Edit curve",
                ButtonKind::Secondary,
                Some(wrap(AdvancedMessage::OpenCurve(line.name.clone()))),
            ),
        };
        let summary = match setting.effective() {
            Some(points) if !points.is_empty() => points_text(points.len()),
            _ => GAME_DEFAULT.to_string(),
        };
        let control = row![
            text(summary).size(size::BODY).style(theme::muted),
            toggle_editor,
            self.reset_button(OptionField::Curve {
                group: line.name.clone(),
            }),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        let line_row = control_row(
            "Response curve",
            notes(setting, |points| points_text(points.len())),
            setting.changed(),
            control,
        );
        match open {
            Some(edit) => column![
                line_row,
                container(self.curve_editor(edit, setting)).padding(Padding {
                    right: space::LG,
                    bottom: space::MD,
                    ..Padding::ZERO
                }),
            ]
            .spacing(space::XS)
            .into(),
            None => line_row,
        }
    }

    fn curve_editor<'a>(
        &'a self,
        edit: &'a CurveEdit,
        setting: &'a Setting<Points>,
    ) -> Element<'a, Message> {
        let writable = self.writable();
        let mut list = Column::new().spacing(space::XS);
        if edit.points.is_empty() {
            list = list.push(hint(NO_POINTS));
        } else {
            list = list.push(
                row![
                    heading("In").width(layout::NUMBER_WIDTH),
                    heading("Out").width(layout::NUMBER_WIDTH),
                ]
                .spacing(space::SM),
            );
        }
        for (index, (x, y)) in edit.points.iter().enumerate() {
            list = list.push(point_row(index, x, y, writable));
        }
        let dirty = Some(&edit.points) != setting.effective();
        list = list.push(
            row![
                button(
                    "Add point",
                    ButtonKind::Secondary,
                    writable.then(|| wrap(AdvancedMessage::AddPoint)),
                ),
                button(
                    "Set curve",
                    ButtonKind::Primary,
                    (writable && dirty).then(|| wrap(AdvancedMessage::SubmitPoints)),
                ),
            ]
            .spacing(space::SM),
        );
        if let Some(error) = edit.error {
            list = list.push(error_text(error));
        }
        let shape = parse_points(&edit.points)
            .or_else(|| setting.effective().and_then(|points| parse_points(points)))
            .unwrap_or_default();
        container(row![curve_plot(&shape), list].spacing(space::XL))
            .padding(space::MD)
            .width(Length::Fill)
            .style(theme::sunken)
            .into()
    }

    fn axes<'a>(&'a self, sheet: &'a Sheet) -> Element<'a, Message> {
        if sheet.blocks.is_empty() {
            return section("Axes", container(hint(NO_AXES)).padding(space::LG));
        }
        let blocks = sheet.blocks.iter().map(|block| -> Element<'a, Message> {
            let mut table = Column::new()
                .push(container(strong(block.title.as_str())).padding(Padding {
                    top: space::MD,
                    right: space::LG,
                    bottom: space::SM,
                    left: space::LG,
                }))
                .push(table_header(&AXIS_COLUMNS));
            for axis in &block.rows {
                table = table.push(self.axis_row(&block.product, axis));
            }
            table.into()
        });
        section("Axes", rows(blocks))
    }

    fn axis_row<'a>(&'a self, product: &str, axis: &'a AxisRow) -> Element<'a, Message> {
        let mut cells = vec![(cell(axis.label.as_str()), AXIS_COLUMNS[0].1)];
        for (at, (field, setting)) in AXIS_FIELDS.iter().zip(&axis.values).enumerate() {
            let key = OptionField::Axis {
                product: product.to_string(),
                input: axis.input.clone(),
                field: *field,
            };
            let content = match field {
                AxisField::Acceleration => self.axis_number(key, setting),
                AxisField::Deadzone | AxisField::Saturation => {
                    let rest = if *field == AxisField::Saturation {
                        1.0
                    } else {
                        0.0
                    };
                    self.axis_slider(key, setting, rest)
                }
            };
            cells.push((content, AXIS_COLUMNS[at + 1].1));
        }
        table_row(cells, false, None)
    }

    fn axis_slider<'a>(
        &'a self,
        field: OptionField,
        setting: &'a Setting<String>,
        rest: f64,
    ) -> Element<'a, Message> {
        let sliding = self.advanced.sliding(&field);
        let value = sliding
            .or_else(|| parse(setting.now.as_ref()))
            .unwrap_or(rest)
            .clamp(0.0, 1.0);
        let shown = sliding.map(number).or_else(|| setting.now.clone());
        let moved = field.clone();
        row![
            slider(0.0..=1.0, value, move |value| {
                wrap(AdvancedMessage::Slide(moved.clone(), value))
            })
            .step(SLIDER_STEP)
            .on_release(wrap(AdvancedMessage::Release))
            .width(metrics::AXIS_SLIDER_WIDTH),
            value_label(shown, setting.staged() || sliding.is_some()),
            self.reset_icon(field),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center)
        .into()
    }

    fn axis_number<'a>(
        &'a self,
        field: OptionField,
        setting: &'a Setting<String>,
    ) -> Element<'a, Message> {
        let value = self
            .advanced
            .draft(&field)
            .or(setting.now.as_deref())
            .unwrap_or_default();
        let typed = field.clone();
        let input = text_field(DEFAULT_VALUE, value, move |text| {
            wrap(AdvancedMessage::Typed(typed.clone(), text))
        })
        .on_submit(wrap(AdvancedMessage::Submit(field.clone())))
        .width(layout::NUMBER_WIDTH)
        .align_x(Alignment::End);
        row![input, self.reset_icon(field)]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into()
    }

    fn reset_icon<'a>(&self, field: OptionField) -> Element<'a, Message> {
        let enabled = self.writable() && self.advanced.held(&field);
        let content: Element<'a, Message> = if enabled {
            icon_only(icons::UNDO, Some(wrap(AdvancedMessage::Reset(field))))
        } else {
            Space::new().into()
        };
        container(content).width(metrics::ICON_BUTTON).into()
    }
}

const AXIS_COLUMNS: [(&str, Length); 4] = [
    ("Axis", Length::Fill),
    ("Deadzone", Length::Fixed(metrics::AXIS_SLIDER_CELL)),
    ("Saturation", Length::Fixed(metrics::AXIS_SLIDER_CELL)),
    ("Acceleration", Length::Fixed(metrics::AXIS_NUMBER_CELL)),
];

fn device_list<'a>(sheet: &'a Sheet, pick: &DevicePick) -> Element<'a, Message> {
    let mut items = Column::new().padding(Padding {
        top: space::XS,
        ..Padding::ZERO
    });
    for entry in &sheet.devices {
        items = items.push(device_item(entry, None, entry.pick == *pick));
    }
    for entry in &sheet.unmapped {
        items = items.push(device_item(entry, Some(NOT_CONNECTED), entry.pick == *pick));
    }
    column![
        table_header(&[("Devices", Length::Fill)]),
        vertical_scroll(items),
    ]
    .into()
}

fn device_item<'a>(
    entry: &'a DeviceEntry,
    note: Option<&'a str>,
    selected: bool,
) -> Element<'a, Message> {
    let mut names = column![label(entry.label.as_str()).wrapping(text::Wrapping::None)];
    if let Some(second) = entry.product.as_deref().or(note) {
        names = names.push(hint(second).wrapping(text::Wrapping::None));
    }
    let mut line = row![names.width(Length::Fill)]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if entry.count > 0 {
        line = line.push(hint(entry.count.to_string()));
    }
    widget::button(line)
        .padding(metrics::LIST_ITEM_PADDING)
        .width(Length::Fill)
        .style(theme::row_button(selected))
        .on_press(wrap(AdvancedMessage::Pick(entry.pick.clone())))
        .into()
}

fn point_row<'a>(index: usize, x: &'a str, y: &'a str, writable: bool) -> Element<'a, Message> {
    let field = |value: &'a str, part: PointPart| {
        number_field(value, move |text| {
            wrap(AdvancedMessage::PointTyped(index, part, text))
        })
        .on_submit(wrap(AdvancedMessage::SubmitPoints))
    };
    row![
        field(x, PointPart::In),
        field(y, PointPart::Out),
        icon_only(
            icons::CLOSE,
            writable.then(|| wrap(AdvancedMessage::RemovePoint(index))),
        ),
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center)
    .into()
}

/// The response as a row of bars, input rising left to right.
fn curve_plot<'a>(points: &[(f64, f64)]) -> Element<'a, Message> {
    let bars = samples(points, PLOT_BARS)
        .into_iter()
        .map(|output| -> Element<'a, Message> {
            let filled = ((output * 100.0).round() as u16).clamp(1, 100);
            column![
                Space::new().height(Length::FillPortion(101 - filled)),
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::FillPortion(filled))
                    .style(theme::plot_bar),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        });
    container(
        Row::with_children(bars)
            .spacing(metrics::PLOT_GAP)
            .height(Length::Fill),
    )
    .padding(space::SM)
    .width(metrics::PLOT_WIDTH)
    .height(metrics::PLOT_HEIGHT)
    .style(theme::card)
    .into()
}

/// A labelled control with its notes underneath and the change marker on the left.
fn control_row<'a>(
    name: &'static str,
    notes: Vec<String>,
    changed: bool,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut labels = Column::new().push(label(name));
    for note in notes {
        labels = labels.push(hint(note));
    }
    container(
        row![
            change_marker(changed),
            labels.width(Length::Fill),
            control.into(),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .padding(metrics::CONTROL_ROW_PADDING)
    .width(Length::Fill)
    .into()
}

/// What a control's notes say: the game's default when the file overrides it, and what the
/// value was before a staged edit.
fn notes<T: PartialEq>(setting: &Setting<T>, show: impl Fn(&T) -> String) -> Vec<String> {
    let mut lines = Vec::new();
    if let (Some(_), Some(default)) = (&setting.now, &setting.default) {
        lines.push(format!("{GAME_DEFAULT}: {}", show(default)));
    }
    if setting.staged() {
        let was = setting
            .was
            .as_ref()
            .map_or_else(|| "not set".to_string(), &show);
        lines.push(format!("Was {was}"));
    }
    lines
}

fn on_off(value: &bool) -> String {
    if *value { "On" } else { "Off" }.to_string()
}

fn points_text(count: usize) -> String {
    if count == 1 {
        "1 point".to_string()
    } else {
        format!("{count} points")
    }
}

fn parse(value: Option<&String>) -> Option<f64> {
    value?.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

fn value_label<'a>(value: Option<String>, staged: bool) -> Element<'a, Message> {
    let set = value.is_some();
    let shown = text(value.unwrap_or_else(|| DEFAULT_VALUE.to_string()))
        .size(size::BODY)
        .width(metrics::VALUE_WIDTH)
        .align_x(Alignment::End)
        .wrapping(text::Wrapping::None);
    let shown = match (staged, set) {
        (true, _) => shown.style(theme::accent),
        (false, false) => shown.style(theme::muted),
        (false, true) => shown,
    };
    shown.into()
}

fn error_text<'a>(error: &'static str) -> Element<'a, Message> {
    text(error)
        .size(size::CAPTION)
        .style(theme::toned(Tone::Danger))
        .into()
}

fn indented(depth: usize, vertical: f32) -> Padding {
    Padding {
        top: vertical,
        right: space::LG,
        bottom: vertical,
        left: space::LG + depth as f32 * metrics::INDENT,
    }
}

fn control_indent(depth: usize) -> f32 {
    space::SM + depth as f32 * metrics::INDENT
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
