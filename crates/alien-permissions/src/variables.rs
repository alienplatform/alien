use crate::{
    error::{ErrorData, Result},
    PermissionContext,
};

/// Utility functions for variable interpolation
pub struct VariableInterpolator;

impl VariableInterpolator {
    /// Interpolate variables in a string template
    pub fn interpolate_variables(template: &str, context: &PermissionContext) -> Result<String> {
        let mut result = template.to_string();

        // Find all variables in the format ${variableName}
        let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();

        for captures in re.captures_iter(template) {
            let full_match = &captures[0];
            let variable_name = &captures[1];

            let value = context.get_variable(variable_name).ok_or_else(|| {
                alien_error::AlienError::new(ErrorData::VariableNotFound {
                    variable: variable_name.to_string(),
                })
            })?;

            result = result.replace(full_match, value);
        }

        Ok(result)
    }

    /// Interpolate variables in a list of strings
    pub fn interpolate_string_list(
        templates: &[String],
        context: &PermissionContext,
    ) -> Result<Vec<String>> {
        templates
            .iter()
            .map(|template| Self::interpolate_variables(template, context))
            .collect()
    }
}
