use crate::error::{ErrorData, Result};
use alien_error::AlienError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationMode {
    Skip,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractionMode {
    json: bool,
    can_prompt: bool,
}

impl InteractionMode {
    pub fn new(json: bool, can_prompt: bool) -> Self {
        Self { json, can_prompt }
    }

    pub fn current(json: bool) -> Self {
        Self::new(json, crate::output::can_prompt())
    }

    pub fn is_machine(self) -> bool {
        self.json || !self.can_prompt
    }

    pub fn require_prompt(self, message: &str) -> Result<()> {
        if self.is_machine() {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: message.to_string(),
            }));
        }

        Ok(())
    }

    pub fn confirmation_mode(self, acknowledged: bool, message: &str) -> Result<ConfirmationMode> {
        if acknowledged {
            return Ok(ConfirmationMode::Skip);
        }

        self.require_prompt(message)?;
        Ok(ConfirmationMode::Prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interaction_mode_detects_machine_mode() {
        assert!(InteractionMode::new(true, true).is_machine());
        assert!(InteractionMode::new(false, false).is_machine());
        assert!(!InteractionMode::new(false, true).is_machine());
    }

    #[test]
    fn require_prompt_rejects_machine_mode() {
        let err = InteractionMode::new(true, true)
            .require_prompt("prompt disabled")
            .unwrap_err();
        assert!(err.to_string().contains("prompt disabled"));
    }

    #[test]
    fn confirmation_mode_skips_when_acknowledged() {
        let mode = InteractionMode::new(false, false);
        assert_eq!(
            mode.confirmation_mode(true, "unused").unwrap(),
            ConfirmationMode::Skip
        );
    }

    #[test]
    fn confirmation_mode_requires_prompt_when_interactive() {
        let mode = InteractionMode::new(false, true);
        assert_eq!(
            mode.confirmation_mode(false, "unused").unwrap(),
            ConfirmationMode::Prompt
        );
    }

    #[test]
    fn confirmation_mode_rejects_machine_without_flag() {
        let err = InteractionMode::new(false, false)
            .confirmation_mode(false, "need flag")
            .unwrap_err();
        assert!(err.to_string().contains("need flag"));
    }
}
