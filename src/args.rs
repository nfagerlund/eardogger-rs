use std::path::PathBuf;
// For the time being, I want to keep this as stupidly simple as possible.
// If it gets out of hand later, I can bring in clap or whatever.

// oh, lol, I already have clap in my cargo.toml. well.... anyway.

/// The --config option lets you specify the path of the config file
/// to use. It's optional; if omitted, we'll use eardogger.toml in the current
/// working directory.
pub fn config_path() -> Option<PathBuf> {
    let mut args = std::env::args();
    let Some(_) = args.find(|a| a == "--config") else {
        return None;
    };
    let Some(p) = args.next() else {
        // This runs before we have a tracing subscriber, so we have to log rudely.
        println!("Startup: received --config without a config path; ignoring!");
        return None;
    };
    Some(PathBuf::from(p))
}
