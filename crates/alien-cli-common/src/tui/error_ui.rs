//! Terminal error printing for AlienError instances
//!
//! This module provides clean, structured error printing directly to the terminal
//! for errors that can be very long (like build outputs) and should not be constrained
//! by TUI display limitations.

#[cfg(feature = "deployment")]
use alien_deployment::ResourceError;
use alien_error::{AlienError, AlienErrorData, GenericError};
use alien_preflights::CheckResult;
use serde_json::Value;
use std::io::{self, Write};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

/// Colors used for terminal error display
pub struct ErrorColors;

impl ErrorColors {
    pub const ERROR_PRIMARY: Color = Color::Red;
    pub const ERROR_SECONDARY: Color = Color::Red;
    pub const WARNING: Color = Color::Yellow;
    pub const CONTEXT: Color = Color::White;
    pub const SUBTLE: Color = Color::Black; // Will be bright black (gray)
    pub const CODE: Color = Color::Green;
    pub const PATH: Color = Color::Blue;
    pub const RESOURCE: Color = Color::Magenta;
}

/// Main error printer for AlienError instances
pub struct ErrorPrinter;

impl ErrorPrinter {
    /// Print any AlienError with full context and metadata to stderr
    pub fn print_alien_error<T>(error: &AlienError<T>, title: Option<&str>) -> io::Result<()>
    where
        T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        let mut stderr = StandardStream::stderr(ColorChoice::Auto);

        writeln!(stderr)?;
        stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
        writeln!(
            stderr,
            "\n═══════════════════════════════ {} ═══════════════════════════════",
            title.unwrap_or("ERROR").to_uppercase()
        )?;
        stderr.reset()?;
        writeln!(stderr)?;

        // Error code as a badge
        stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::ERROR_PRIMARY)))?;
        write!(stderr, "● ")?;
        stderr.set_color(
            ColorSpec::new()
                .set_fg(Some(ErrorColors::ERROR_PRIMARY))
                .set_bold(true),
        )?;
        writeln!(stderr, "{}", error.code)?;
        stderr.reset()?;

        // Main error message
        write!(stderr, "  ")?;
        stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::ERROR_SECONDARY)))?;
        writeln!(stderr, "{}", error.message)?;
        stderr.reset()?;

        // Special handling for specific error types
        Self::print_specialized_error_details(&mut stderr, error)?;

        // Print error chain if present using the same format
        if let Some(ref source) = error.source {
            Self::print_all_errors_flat(&mut stderr, source)?;
        }

        // Help section if retryable
        // if error.retryable {
        //     writeln!(stderr)?;
        //     stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::WARNING)))?;
        //     write!(stderr, "💡 ")?;
        //     writeln!(stderr, "This error is retryable - try running the command again")?;
        //     stderr.reset()?;
        // }

        writeln!(stderr)?;
        // stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
        stderr.reset()?;

        Ok(())
    }

    /// Print an AlienError with full context and metadata to stderr (for backward compatibility)
    pub fn print_error(error: &AlienError<GenericError>, title: Option<&str>) -> io::Result<()> {
        Self::print_alien_error(error, title)
    }

    /// Handle specialized error types with custom formatting
    fn print_specialized_error_details<T>(
        stderr: &mut StandardStream,
        error: &AlienError<T>,
    ) -> io::Result<()>
    where
        T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        // Try to match specific error codes and provide specialized formatting
        match error.code.as_str() {
            "VALIDATION_FAILED" => {
                Self::print_preflight_validation_details(stderr, error)?;
            }
            "MULTIPLE_FUNCTION_BUILDS_FAILED" => {
                Self::print_multiple_builds_details(stderr, error)?;
            }
            #[cfg(feature = "deployment")]
            "AGENT_DEPLOYMENT_FAILED" => {
                Self::print_agent_deployment_failed_details(stderr, error)?;
            }
            _ => {
                // Fallback to generic context printing
                if let Some(ref context) = error.context {
                    Self::print_context_as_error_details(stderr, context)?;
                }
            }
        }
        Ok(())
    }

    /// Print preflight validation errors and warnings in detail
    fn print_preflight_validation_details<T>(
        stderr: &mut StandardStream,
        error: &AlienError<T>,
    ) -> io::Result<()>
    where
        T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        if let Some(ref context) = error.context {
            if let Some(results_value) = context.get("results") {
                if let Ok(results) =
                    serde_json::from_value::<Vec<CheckResult>>(results_value.clone())
                {
                    for result in results.iter() {
                        // Only show failed checks and warnings, skip pure successes
                        if !result.success || !result.warnings.is_empty() {
                            writeln!(stderr)?;

                            // Get the check description from the result, or use a fallback
                            let check_description = result
                                .check_description
                                .as_deref()
                                .unwrap_or("Unknown check");

                            if result.success && !result.warnings.is_empty() {
                                // Success with warnings
                                stderr.set_color(
                                    ColorSpec::new().set_fg(Some(Color::Yellow)).set_bold(true),
                                )?;
                                writeln!(stderr, "  ⚠ {}:", check_description)?;
                                stderr.reset()?;
                                for warning_msg in &result.warnings {
                                    stderr
                                        .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
                                    writeln!(stderr, "    • {}", warning_msg)?;
                                }
                            } else if !result.success && result.warnings.is_empty() {
                                // Pure failure
                                stderr.set_color(
                                    ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true),
                                )?;
                                writeln!(stderr, "  ✗ {}:", check_description)?;
                                stderr.reset()?;
                                for error_msg in &result.errors {
                                    stderr
                                        .set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
                                    writeln!(stderr, "    • {}", error_msg)?;
                                }
                            } else {
                                // Failed with warnings
                                stderr.set_color(
                                    ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true),
                                )?;
                                writeln!(stderr, "  ✗ {}:", check_description)?;
                                stderr.reset()?;
                                for error_msg in &result.errors {
                                    stderr
                                        .set_color(ColorSpec::new().set_fg(Some(Color::White)))?;
                                    writeln!(stderr, "    • {}", error_msg)?;
                                }
                                if !result.warnings.is_empty() {
                                    stderr
                                        .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
                                    writeln!(stderr, "    Warnings:")?;
                                    for warning_msg in &result.warnings {
                                        stderr.set_color(
                                            ColorSpec::new().set_fg(Some(Color::Yellow)),
                                        )?;
                                        writeln!(stderr, "    • {}", warning_msg)?;
                                    }
                                }
                            }
                            stderr.reset()?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Print multiple function build failure details
    fn print_multiple_builds_details<T>(
        stderr: &mut StandardStream,
        error: &AlienError<T>,
    ) -> io::Result<()>
    where
        T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        if let Some(ref context) = error.context {
            // Print the summary information in a more readable format
            writeln!(stderr)?;
            stderr.set_color(
                ColorSpec::new()
                    .set_fg(Some(ErrorColors::SUBTLE))
                    .set_bold(true),
            )?;
            writeln!(stderr, "Build Summary:")?;
            stderr.reset()?;

            if let Some(failed_functions) = context.get("failed_functions") {
                if let Ok(functions) =
                    serde_json::from_value::<Vec<String>>(failed_functions.clone())
                {
                    stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::ERROR_PRIMARY)))?;
                    writeln!(stderr, "  Failed Functions:")?;
                    stderr.reset()?;
                    for function in functions {
                        stderr.set_color(
                            ColorSpec::new().set_fg(Some(ErrorColors::ERROR_SECONDARY)),
                        )?;
                        writeln!(stderr, "    ✗ {}", function)?;
                    }
                    stderr.reset()?;
                }
            }

            if let Some(successful_functions) = context.get("successful_functions") {
                if let Ok(functions) =
                    serde_json::from_value::<Vec<String>>(successful_functions.clone())
                {
                    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                    writeln!(stderr, "  Successful Functions:")?;
                    stderr.reset()?;
                    for function in functions {
                        stderr.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                        writeln!(stderr, "    ✓ {}", function)?;
                    }
                    stderr.reset()?;
                }
            }

            writeln!(stderr)?;
            stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::SUBTLE)))?;
            stderr.reset()?;
        }
        Ok(())
    }

    /// Print agent deployment failure details with resource-level errors
    #[cfg(feature = "deployment")]
    fn print_agent_deployment_failed_details<T>(
        stderr: &mut StandardStream,
        error: &AlienError<T>,
    ) -> io::Result<()>
    where
        T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        if let Some(ref context) = error.context {
            writeln!(stderr)?;

            if let Some(resource_errors_value) = context.get("resource_errors") {
                if let Ok(resource_errors) =
                    serde_json::from_value::<Vec<ResourceError>>(resource_errors_value.clone())
                {
                    for res_error in resource_errors {
                        stderr.set_color(
                            ColorSpec::new().set_fg(Some(ErrorColors::ERROR_SECONDARY)),
                        )?;
                        write!(stderr, "  ✗ ")?;
                        stderr.set_color(
                            ColorSpec::new()
                                .set_fg(Some(ErrorColors::RESOURCE))
                                .set_bold(true),
                        )?;
                        write!(stderr, "{}", res_error.resource_id)?;
                        stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::SUBTLE)))?;
                        writeln!(stderr, " ({})", res_error.resource_type)?;
                        stderr.reset()?;

                        // If there's an error object with details, print it
                        if let Some(ref err) = res_error.error {
                            stderr
                                .set_color(ColorSpec::new().set_fg(Some(ErrorColors::CONTEXT)))?;
                            writeln!(stderr, "     {}", err.message)?;
                            stderr.reset()?;

                            // If there's a source error chain, show it too
                            if let Some(ref source) = err.source {
                                stderr.set_color(
                                    ColorSpec::new().set_fg(Some(ErrorColors::SUBTLE)),
                                )?;
                                writeln!(stderr, "     └─ {}: {}", source.code, source.message)?;
                                stderr.reset()?;
                            }
                        }
                    }
                }
            }

            writeln!(stderr)?;
        }
        Ok(())
    }

    /// Print context as error details (for errors created with AlienError::new)
    fn print_context_as_error_details(
        stderr: &mut StandardStream,
        context: &Value,
    ) -> io::Result<()> {
        if let Some(error_obj) = context.as_object() {
            // Define the desired field order - verbose fields like build_output should come last
            let field_order = [
                "function_name",
                "resource_id",
                "reason",
                "message",
                "platform",
                "operation",
                "file_path",
                "stack_name",
                "check_description",
                "build_output", // This should always be last as it's most verbose
            ];

            // Collect and sort fields according to our desired order
            let mut fields: Vec<_> = error_obj.iter().collect();
            fields.sort_by_key(|(key, _)| {
                field_order
                    .iter()
                    .position(|&field| field == *key)
                    .unwrap_or(field_order.len())
            });

            // Only show fields that contain useful detail information
            for (key, value) in fields {
                // Skip internal/metadata fields and focus on user-relevant details
                if Self::is_user_relevant_field(key) {
                    let formatted_value = Self::format_error_field_value(value);
                    if !formatted_value.is_empty() {
                        write!(stderr, "  ")?;
                        stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::RESOURCE)))?;
                        write!(stderr, "• ")?;
                        stderr.set_color(
                            ColorSpec::new()
                                .set_fg(Some(ErrorColors::SUBTLE))
                                .set_bold(true),
                        )?;
                        write!(stderr, "{}: ", Self::format_field_name(key))?;
                        stderr.reset()?;

                        // Check if the value contains newlines
                        if formatted_value.contains('\n') {
                            // Multi-line: show field name, then indented content on new lines
                            writeln!(stderr)?;
                            stderr
                                .set_color(ColorSpec::new().set_fg(Some(ErrorColors::CONTEXT)))?;
                            for line in formatted_value.lines() {
                                writeln!(stderr, "     {}", line)?;
                            }
                            stderr.reset()?;
                        } else {
                            // Single line: show on same line
                            stderr
                                .set_color(ColorSpec::new().set_fg(Some(ErrorColors::CONTEXT)))?;
                            writeln!(stderr, "{}", formatted_value)?;
                            stderr.reset()?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a field contains user-relevant information
    fn is_user_relevant_field(field_name: &str) -> bool {
        // Exclude the "message" field since it's already shown as the main error message
        // Include all other fields that provide actionable details to users
        field_name != "message"
    }

    /// Format error field values for display
    fn format_error_field_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                if items.len() <= 10 {
                    items.join(", ")
                } else {
                    format!("{} (+{} more)", items[..10].join(", "), items.len() - 10)
                }
            }
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => String::new(), // Skip complex objects
        }
    }

    /// Format field names for better readability
    fn format_field_name(field_name: &str) -> String {
        // Convert snake_case to human-readable format
        field_name
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Print all errors in the chain with the same flat format  
    fn print_all_errors_flat(
        stderr: &mut StandardStream,
        error: &AlienError<GenericError>,
    ) -> io::Result<()> {
        let mut current_error = Some(error);

        while let Some(err) = current_error {
            writeln!(stderr)?;
            writeln!(stderr)?;

            // Error code as a badge
            stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::ERROR_PRIMARY)))?;
            write!(stderr, "● ")?;
            stderr.set_color(
                ColorSpec::new()
                    .set_fg(Some(ErrorColors::ERROR_PRIMARY))
                    .set_bold(true),
            )?;
            writeln!(stderr, "{}", err.code)?;
            stderr.reset()?;

            // Main error message
            write!(stderr, "  ")?;
            stderr.set_color(ColorSpec::new().set_fg(Some(ErrorColors::ERROR_SECONDARY)))?;
            writeln!(stderr, "{}", err.message)?;
            stderr.reset()?;

            // Special handling for specific error types (this will handle context printing)
            Self::print_specialized_error_details(stderr, err)?;

            current_error = err.source.as_deref();
        }

        Ok(())
    }
}
