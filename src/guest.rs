//! The files the builder writes into the guest, every one of them.
//!
//! Each is a real file under `guest/` (so it can be read, run and syntax-checked
//! as shell), with `@NAME@` placeholders for values only known at build time.
//! [`render`] fills them and refuses a template with a placeholder it was not
//! given. Recipes never render a guest file: they get the rendered text as a
//! named input.

use anyhow::{bail, Result};

/// A file under `guest/`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuestFile {
    /// The path under `guest/`.
    pub(crate) path: &'static str,
    pub(crate) template: &'static str,
}

macro_rules! guest_files {
    ($($name:ident = $path:literal;)*) => {
        $(pub(crate) const $name: GuestFile = GuestFile {
            path: $path,
            template: include_str!(concat!("../guest/", $path)),
        };)*
    };
}

guest_files! {
    INSTALL_UX = "install-ux/install-ux.sh";
    LIVE_HELP = "live/live-help.sh";
    LIVE_SHELL = "live/shell-ux.sh";
    LIVE_SHUTDOWN_CLEANUP = "live/live-shutdown-cleanup.sh";
    SYSTEM_SETUP = "installed/system-setup.sh";
}

/// A placeholder's value, which carries its own quoting.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Value<'a> {
    /// One shell word that needs no quoting (a path, a name, a variable
    /// name); anything else is refused.
    Word(&'a str),
    /// A command line, each word quoted where the shell needs it.
    Argv(&'a [String]),
    /// One line of text in a quoted heredoc or a comment, as it is.
    Text(&'a str),
}

/// Whether `word` is a shell word that means itself unquoted.
pub(crate) fn is_word(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/-_.=:+,".contains(c))
}

/// `argv` as one POSIX shell command line, each word single-quoted where needed.
pub(crate) fn shell_words(argv: &[String]) -> String {
    argv.iter()
        .map(|word| {
            if is_word(word) {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Value<'_> {
    fn text(self, name: &str, file: &GuestFile) -> Result<String> {
        Ok(match self {
            Value::Word(word) if is_word(word) => word.to_string(),
            Value::Word(word) => {
                bail!("@{name}@ in guest/{} must be one plain shell word, not {word:?}", file.path)
            }
            Value::Argv(argv) => shell_words(argv),
            Value::Text(text) if !text.contains('\n') => text.to_string(),
            Value::Text(text) => {
                bail!("@{name}@ in guest/{} must be one line, not {text:?}", file.path)
            }
        })
    }
}

/// `file` with every `@NAME@` (a name of letters, digits and `_`, in any
/// case) filled from `values`. A placeholder with no value is an error, so
/// a misspelt name never reaches the guest as literal text; values the file
/// does not use are ignored.
pub(crate) fn render(file: &GuestFile, values: &[(&str, Value)]) -> Result<String> {
    let mut out = String::with_capacity(file.template.len());
    let mut rest = file.template;
    while let Some(start) = rest.find('@') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if name_len > 0 && after[name_len..].starts_with('@') {
            let name = &after[..name_len];
            let Some((_, value)) = values.iter().find(|(key, _)| *key == name) else {
                bail!("placeholder @{name}@ in guest/{} has no value", file.path);
            };
            out.push_str(&value.text(name, file)?);
            rest = &after[name_len + 1..];
        } else {
            out.push('@');
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(out)
}
