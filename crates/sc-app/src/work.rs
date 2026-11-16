use iced::Task;
use std::path::PathBuf;

/// Runs blocking disk or process work on tokio's blocking pool and maps the result to a message.
pub fn background<T, M>(
    job: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) -> M + Send + 'static,
) -> Task<M>
where
    T: Send + 'static,
    M: Send + 'static,
{
    Task::perform(
        async move { tokio::task::spawn_blocking(job).await },
        move |joined| match joined {
            Ok(value) => done(value),
            Err(err) => std::panic::resume_unwind(err.into_panic()),
        },
    )
}

/// Opens the system folder picker.
pub fn pick_folder<M: Send + 'static>(
    title: &str,
    done: impl FnOnce(Option<PathBuf>) -> M + Send + 'static,
) -> Task<M> {
    let dialog = rfd::AsyncFileDialog::new().set_title(title);
    Task::perform(
        async move {
            dialog
                .pick_folder()
                .await
                .map(|handle| handle.path().to_path_buf())
        },
        done,
    )
}

/// Opens a file or folder with the desktop's default program.
pub fn open_path<M: Send + 'static>(
    path: PathBuf,
    done: impl FnOnce(Result<(), String>) -> M + Send + 'static,
) -> Task<M> {
    background(
        move || open::that_detached(&path).map_err(|err| err.to_string()),
        done,
    )
}
