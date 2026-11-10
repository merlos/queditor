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
    about = "Markdown/text editor with multi-provider AI and safe edit review"
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
            "Queditor {} · {} · {} · {} agents · {} backend ({})",
            env!("CARGO_PKG_VERSION"),
            app.config.theme,
            app.config.keybindings,
            app.agents.len(),
            app.backend_label(),
            app.config.model
        );
        println!("{}", app.status);
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("Queditor needs an interactive terminal. Use --check for a startup check.");
    }
    let runtime = tokio::runtime::Runtime::new().context("Cannot initialize async runtime")?;
    let interrupted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(signal, interrupted.clone())
            .context("Cannot install shutdown signal handler")?;
    }
    let store = match queditor::storage::Store::new(queditor::storage::state_dir()) {
        Ok(store) => Some(store),
        Err(_) => {
            app.notify(
                queditor::app::Level::Warning,
                "Local state storage is unavailable; recovery and persistent permissions disabled",
            );
            None
        }
    };
    app.enable_live(runtime.handle().clone(), store);
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
    let mut checkpoint = std::time::Instant::now();
    let mut animation = std::time::Instant::now();
    let mut state_changed = false;
    let mut preserve_recovery = false;
    while !app.quit {
        if interrupted.load(std::sync::atomic::Ordering::Relaxed) {
            app.cancel_live();
            app.checkpoint_state()
                .context("Could not checkpoint interrupted work")?;
            preserve_recovery = true;
            break;
        }
        let mut redraw = app.poll_live();
        if app.busy() && animation.elapsed() >= Duration::from_millis(100) {
            app.animation_frame = app.animation_frame.wrapping_add(1);
            animation = std::time::Instant::now();
            redraw = true;
        }
        if redraw {
            state_changed = true;
        }
        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.key(key),
                Event::Mouse(mouse) => app.mouse(mouse),
                Event::Paste(text) => app.paste(&text),
                Event::Resize(_, _) => app.follow_editor = true,
                _ => {}
            }
            redraw = true;
            state_changed = true;
        }
        if state_changed && checkpoint.elapsed() >= Duration::from_secs(1) {
            if app.checkpoint_state().is_err() {
                app.notify(
                    queditor::app::Level::Warning,
                    "Local checkpoint failed; save the document explicitly",
                );
                redraw = true;
            }
            checkpoint = std::time::Instant::now();
            state_changed = false;
        }
        if redraw && !app.quit {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
        }
    }
    if !preserve_recovery {
        app.finish_state()
            .context("Could not finish local session/recovery state")?;
    }
    Ok(())
}
