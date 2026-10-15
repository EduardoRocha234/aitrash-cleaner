use anstyle::{AnsiColor, Style};

const RED: Style = AnsiColor::Red.on_default().bold();
const BOLD: Style = Style::new().bold();

/// (broom, wordmark) per line; the broom column has a fixed width of 11 cells.
const LINES: [(&str, &str); 5] = [
    ("      ╱", ""),
    ("    ▗▟▘", ""),
    ("   ▟██▙    ", "┏━┓╻╺┳╸┏━┓┏━┓┏━┓╻ ╻   ┏━╸╻  ┏━╸┏━┓┏┓╻┏━╸┏━┓"),
    ("  ▟████▘ ✦ ", "┣━┫┃ ┃ ┣┳┛┣━┫┗━┓┣━┫╺━╸┃  ┃  ┣╸ ┣━┫┃┗┫┣╸ ┣┳┛"),
    (" ▀ ▀ ▀     ", "╹ ╹╹ ╹ ╹┗╸╹ ╹┗━┛╹ ╹   ┗━╸┗━╸┗━╸╹ ╹╹ ╹┗━╸╹┗╸"),
];

/// Columns needed to draw the logo without wrapping.
pub const WIDTH: usize = 58;

/// Red broom + bold wordmark; ANSI codes are stripped by anstream when color is off.
pub fn render() -> String {
    LINES
        .iter()
        .map(|(broom, text)| {
            if text.is_empty() {
                format!("{RED}{broom}{RED:#}")
            } else {
                format!("{RED}{broom}{RED:#}{BOLD}{text}{BOLD:#}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Hide the logo on terminals too narrow for it.
pub fn fits() -> bool {
    dialoguer::console::Term::stdout().size_checked().is_none_or(|(_, cols)| cols as usize >= WIDTH)
}
