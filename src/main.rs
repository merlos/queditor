use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use queditor::{app::App, ui};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Markdown/text editor with a mock assistant and safe edit review"
)]
struct Args {
    /// UTF-8 file to open (a missing path creates a new document)
    file: Option<PathBuf>,
    /// Check startup/configuration without entering the terminal UI
    #[arg(long)]
    check: bool,
}

/// Restores the terminal on ordinary errors and unwinding panics.
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}
fn main() -> Result<()> {
    let args = Args::parse();
    let mut app = App::new(args.file.as_deref())?;
    if args.check {
        println!(
            "Queditor {} · {} · {} · {} agents · mock backend",
            env!("CARGO_PKG_VERSION"),
            app.config.theme,
            app.config.keybindings,
            app.agents.len()
        );
        println!("{}", app.status);
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("Queditor needs an interactive terminal. Use --check for a startup check.");
    }
    enable_raw_mode().context("Cannot enable terminal raw mode")?;
    let _guard = TerminalGuard;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    // Restore before the panic hook prints diagnostics; Drop restores again harmlessly.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        previous_hook(info);
    }));
    terminal.draw(|frame| ui::draw(frame, &mut app))?;
    while !app.quit {
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.key(key),
                Event::Mouse(mouse) => app.mouse(mouse),
                Event::Paste(text) => app.paste(&text),
                Event::Resize(_, _) => app.follow_editor = true,
                _ => {}
            }
            if !app.quit {
                terminal.draw(|frame| ui::draw(frame, &mut app))?;
            }
        }
    }
    Ok(())
}
