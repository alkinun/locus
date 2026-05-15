use indicatif::{ProgressBar, ProgressStyle};

const PROGRESS_TEMPLATE: &str =
    "{msg} [{elapsed_precise}] [{wide_bar}] {pos}/{len} {per_sec} ({eta})";

pub fn progress_bar(message: impl Into<String>, total: usize) -> ProgressBar {
    let progress_bar = ProgressBar::new(total as u64);
    progress_bar.set_message(message.into());
    progress_bar.set_style(
        ProgressStyle::with_template(PROGRESS_TEMPLATE).expect("valid progress bar template"),
    );
    progress_bar
}
