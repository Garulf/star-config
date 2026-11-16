use crate::context::Context;
use crate::screen::{Action, Screen};
use iced::{Element, Task};

pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {}

impl State {
    pub fn new() -> Self {
        Self {}
    }

    pub fn reload(&mut self, _ctx: &Context) -> Task<Message> {
        Task::none()
    }

    pub fn update(&mut self, message: Message, _ctx: &Context) -> Action<Message> {
        match message {}
    }

    pub fn view<'a>(&'a self, _ctx: &'a Context) -> Element<'a, Message> {
        super::placeholder(
            Screen::Screenshots,
            "Browse, archive and share your screenshots.",
        )
    }

    pub fn pending(&self) -> usize {
        0
    }

    pub fn apply(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }

    pub fn discard(&mut self) {}

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
