extern crate self as sc_app;

pub mod app;
pub mod context;
pub mod icons;
pub mod screen;
pub mod screens;
pub mod theme;
pub mod widgets;
pub mod work;

#[cfg(test)]
mod testkit;

use app::App;
use context::Context;
use iced::widget::{button, center, column, text};
use iced::{Alignment, Element, Size, window};
use sc_core::dirs::AppDirs;
use sc_core::lock::InstanceLock;
use theme::{layout, size, space};

/// Fonts and text defaults shared by the app window and the headless tests.
pub fn settings() -> iced::Settings {
    iced::Settings {
        fonts: vec![theme::FONT_SEMIBOLD_BYTES.into()],
        default_font: theme::FONT,
        default_text_size: size::BODY.into(),
        ..iced::Settings::default()
    }
}

pub fn run() -> iced::Result {
    let Some(dirs) = AppDirs::from_system() else {
        return notice("star-config cannot find its config folder.".to_string());
    };
    let lock = match InstanceLock::acquire(&dirs) {
        Ok(Some(lock)) => lock,
        Ok(None) => return notice("star-config is already open".to_string()),
        Err(err) => return notice(format!("star-config cannot start: {err}")),
    };
    let ctx = match Context::from_system(dirs) {
        Ok(ctx) => ctx,
        Err(err) => {
            return notice(format!(
                "star-config cannot read its settings, so it did not start. Nothing was changed. {err}"
            ));
        }
    };
    let result = iced::application(move || App::boot(ctx.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .settings(settings())
        .window(window::Settings {
            size: Size::new(layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT),
            min_size: Some(Size::new(
                layout::WINDOW_MIN_WIDTH,
                layout::WINDOW_MIN_HEIGHT,
            )),
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                application_id: String::from("star-config"),
                ..Default::default()
            },
            ..window::Settings::default()
        })
        .run();
    drop(lock);
    result
}

#[derive(Debug, Clone)]
struct Close;

struct Notice(String);

fn notice(message: String) -> iced::Result {
    iced::application(
        move || Notice(message.clone()),
        |_: &mut Notice, _: Close| iced::exit::<Close>(),
        notice_view,
    )
    .title("star-config")
    .theme(|_: &Notice| theme::theme(Default::default()))
    .settings(settings())
    .window(window::Settings {
        size: Size::new(layout::NOTICE_WIDTH, layout::NOTICE_HEIGHT),
        resizable: false,
        ..window::Settings::default()
    })
    .run()
}

fn notice_view(notice: &Notice) -> Element<'_, Close> {
    center(
        column![
            text(notice.0.as_str()).size(size::BODY),
            button(text("Close").size(size::BODY))
                .padding([space::SM, space::LG])
                .style(theme::primary)
                .on_press(Close),
        ]
        .spacing(space::LG)
        .align_x(Alignment::Center),
    )
    .style(theme::window)
    .into()
}
