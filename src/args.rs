use std::path::PathBuf;
// For the time being, I want to keep this as stupidly simple as possible.
// If it gets out of hand later, I can bring in clap or whatever.

// oh, lol, I already have clap in my cargo.toml. well.... anyway.

pub struct Options {
    /// --config lets you specify the path of the config file to use.
    /// It's optional; if omitted, we'll use eardogger.toml in the current
    /// working directory.
    pub config: Option<PathBuf>,
    /// --migrate runs any pending database migrations, and then exits instead
    /// of starting the server.
    pub migrate: bool,
    /// --status prints the current database migrations status and then exits.
    pub status: bool,
}

enum ParserState {
    Scanning,
    ConfigVal,
}

pub fn cli_options() -> Options {
    let mut config = None;
    let mut migrate = false;
    let mut status = false;

    let mut state = ParserState::Scanning;
    for arg in std::env::args() {
        match state {
            ParserState::Scanning => {
                // I think the correct thing would be to do a tokenization pass
                // first and then do an exhaustive match on token kind. But once I'm
                // considering that, it's time to roll in clap.
                if arg == "--migrate" {
                    migrate = true;
                } else if arg == "--config" {
                    state = ParserState::ConfigVal;
                } else if arg == "--status" {
                    status = true;
                }
                // otherwise ignore.
            }
            ParserState::ConfigVal => {
                config = Some(PathBuf::from(arg));
                state = ParserState::Scanning;
            }
        }
    }
    // cleanup, once all args are consumed
    match state {
        ParserState::Scanning => (),
        ParserState::ConfigVal => {
            // This runs before we have a tracing subscriber, so we have to log rudely.
            println!("Startup: received --config without a config path; ignoring!");
        }
    }

    Options {
        config,
        migrate,
        status,
    }
}
