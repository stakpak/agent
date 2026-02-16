//! Predefined slash commands embedded in the binary. Any `.md` here becomes `/name`.

use include_dir::{Dir, include_dir};
use once_cell::sync::Lazy;

static SLASH_COMMANDS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/slash_commands");

pub static PREDEFINED_COMMANDS: Lazy<Vec<(&'static str, &'static str)>> = Lazy::new(|| {
    SLASH_COMMANDS_DIR
        .files()
        .filter(|f| f.path().extension().is_some_and(|e| e == "md"))
        .filter_map(|f| {
            let name = f.path().file_stem()?.to_str()?;
            let content = f.contents_utf8()?.trim();
            Some((name, content))
        })
        .collect()
});
