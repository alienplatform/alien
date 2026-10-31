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

/// Distinguish IAM request variables from setup placeholders, without validating IAM semantics.
pub(crate) fn is_iam_policy_variable(variable: &str) -> bool {
    let key = variable
        .split_once(',')
        .map_or(variable, |(key, _)| key)
        .trim();
    let iam_key = key.split_once(':').is_some_and(|(namespace, name)| {
        !namespace.is_empty()
            && !name.is_empty()
            && !name.starts_with(':')
            && namespace
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
            && !name.contains(['$', '{', '}'])
    });
    iam_key || matches!(variable, "*" | "?" | "$")
}

/// Resolve setup inputs while leaving IAM request variables for AWS to evaluate.
pub(crate) fn interpolate_aws_variables(
    template: &str,
    context: &PermissionContext,
) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((literal, rest)) = remaining.split_once("${") {
        result.push_str(literal);
        let (variable, suffix) = rest.split_once('}').ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::InvalidPermissionSet {
                message: "AWS policy contains an unterminated variable".to_string(),
            })
        })?;
        if is_iam_policy_variable(variable) {
            result.push_str("${");
            result.push_str(variable);
            result.push('}');
        } else {
            result.push_str(context.get_variable(variable).ok_or_else(|| {
                alien_error::AlienError::new(ErrorData::VariableNotFound {
                    variable: variable.to_string(),
                })
            })?);
        }
        remaining = suffix;
    }
    result.push_str(remaining);
    Ok(result)
}
