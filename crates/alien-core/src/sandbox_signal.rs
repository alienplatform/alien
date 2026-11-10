//! The signals a caller may send to an interactive sandbox command.
//!
//! A closed set rather than a number: signal numbers differ between kernels and architectures,
//! and a caller that could name any number could also name one that stops the agent's own
//! supervision (`SIGSTOP`) rather than the command. Every name here means the same thing on every
//! backend, and each backend maps it at the last moment.

use serde::{Deserialize, Serialize};

/// A signal for an interactive command. It reaches the command's whole process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum CommandSignal {
    /// `SIGINT`, what Ctrl-C sends: a REPL abandons the current input and keeps running
    Interrupt,
    /// `SIGTERM`, a request to exit
    Terminate,
    /// `SIGKILL`, which cannot be caught
    Kill,
    /// `SIGHUP`
    Hangup,
    /// `SIGQUIT`
    Quit,
    /// `SIGUSR1`
    User1,
    /// `SIGUSR2`
    User2,
}

impl CommandSignal {
    /// The name `kill -s` takes, for backends that signal through a shell inside the sandbox.
    pub fn posix_name(self) -> &'static str {
        match self {
            Self::Interrupt => "INT",
            Self::Terminate => "TERM",
            Self::Kill => "KILL",
            Self::Hangup => "HUP",
            Self::Quit => "QUIT",
            Self::User1 => "USR1",
            Self::User2 => "USR2",
        }
    }

    /// Every signal, for callers that validate a name or test each one.
    pub const ALL: [Self; 7] = [
        Self::Interrupt,
        Self::Terminate,
        Self::Kill,
        Self::Hangup,
        Self::Quit,
        Self::User1,
        Self::User2,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire names are the contract with every binding and agent; a rename here breaks an agent
    /// already baked into an image.
    #[test]
    fn signals_serialize_as_their_wire_names() {
        let names: Vec<String> = CommandSignal::ALL
            .iter()
            .map(|signal| {
                serde_json::to_value(signal)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "interrupt",
                "terminate",
                "kill",
                "hangup",
                "quit",
                "user1",
                "user2"
            ]
        );
    }

    #[test]
    fn an_unknown_signal_is_refused() {
        assert!(serde_json::from_str::<CommandSignal>("\"stop\"").is_err());
    }
}
