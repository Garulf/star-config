macro_rules! icons {
    ($($name:ident => $file:literal),* $(,)?) => {
        $(pub const $name: &[u8] = include_bytes!(concat!("../assets/icons/", $file, ".svg"));)*
    };
}

icons! {
    OVERVIEW => "layout-dashboard",
    CONFIG => "sliders-horizontal",
    KEYBINDS => "keyboard",
    COMMS => "headset",
    STARSTRINGS => "languages",
    SCREENSHOTS => "image",
    BACKUPS => "archive",
    MAINTENANCE => "wrench",
    CHANNELS => "git-branch",
    SETTINGS => "settings",
    LAUNCH => "rocket",
    FOLDER => "folder",
    CHECK => "check",
    WARNING => "triangle-exclamation-point",
    CLOSE => "x",
    UNDO => "undo-2",
    SEARCH => "search",
    INFO => "info",
}
