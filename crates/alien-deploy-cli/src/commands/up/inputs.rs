use super::*;

pub(super) fn stack_input_matches_context(
    input: &StackInputDefinition,
    platform: Platform,
) -> bool {
    if !input.provided_by.contains(&StackInputProvider::Deployer) {
        return false;
    }
    if let Some(platforms) = &input.platforms {
        if !platforms.contains(&platform) {
            return false;
        }
    }
    true
}

pub(super) fn collect_deployer_input_values(
    inputs: &[StackInputDefinition],
    input_values: &[String],
    secret_input_values: &[String],
    deploy_config: Option<&DeployConfigFile>,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut raw_values = HashMap::<String, String>::new();

    if let Some(config_inputs) = deploy_config.and_then(|config| config.inputs.as_ref()) {
        for (id, value) in config_inputs {
            raw_values.insert(id.clone(), value.clone());
        }
    }
    if let Some(config_inputs) = deploy_config.and_then(|config| config.secret_inputs.as_ref()) {
        for (id, value) in config_inputs {
            raw_values.insert(id.clone(), value.clone());
        }
    }
    for input in input_values {
        let (id, value) = parse_stack_input_arg(input, "--input")?;
        raw_values.insert(id, value);
    }
    for input in secret_input_values {
        let (id, value) = parse_stack_input_arg(input, "--secret-input")?;
        raw_values.insert(id, value);
    }

    if inputs.is_empty() {
        return Ok(raw_values
            .into_iter()
            .map(|(id, value)| (id, serde_json::Value::String(value)))
            .collect());
    }

    for id in raw_values.keys() {
        if !inputs.iter().any(|input| input.id == *id) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("Unknown or unavailable deployer stack input '{id}'."),
            }));
        }
    }

    for input in inputs.iter().filter(|input| input.required) {
        if raw_values.contains_key(&input.id) {
            continue;
        }
        if !can_prompt() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!(
                    "Missing deployer input: {}. Pass {} {}=... or add [{}] to deployment.toml.",
                    input.label,
                    if matches!(input.kind, StackInputKind::Secret) {
                        "--secret-input"
                    } else {
                        "--input"
                    },
                    input.id,
                    if matches!(input.kind, StackInputKind::Secret) {
                        "secretInputs"
                    } else {
                        "inputs"
                    }
                ),
            }));
        }
        let value = prompt_input_value(input)?;
        raw_values.insert(input.id.clone(), value);
    }

    let mut values = HashMap::new();
    for input in inputs {
        let Some(raw_value) = raw_values.get(&input.id) else {
            continue;
        };
        values.insert(input.id.clone(), parse_stack_input_value(input, raw_value)?);
    }
    Ok(values)
}

fn parse_stack_input_arg(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: '{input}'. Use id=value"),
        }));
    };
    if id.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: input id is required"),
        }));
    }
    Ok((id.trim().to_string(), value.to_string()))
}

fn parse_stack_input_value(input: &StackInputDefinition, value: &str) -> Result<serde_json::Value> {
    match input.kind {
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => {
            validate_string_stack_input(input, value)?;
            Ok(serde_json::Value::String(value.to_string()))
        }
        StackInputKind::Number => {
            let number = value.parse::<f64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a number.", input.label),
                })
            })?;
            serde_json::Number::from_f64(number)
                .map(serde_json::Value::Number)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ValidationError {
                        field: input.id.clone(),
                        message: format!("{} must be a finite number.", input.label),
                    })
                })
        }
        StackInputKind::Integer => {
            let number = value.parse::<i64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a whole number.", input.label),
                })
            })?;
            Ok(serde_json::Value::Number(number.into()))
        }
        StackInputKind::Boolean => {
            let parsed = value.parse::<bool>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be true or false.", input.label),
                })
            })?;
            Ok(serde_json::Value::Bool(parsed))
        }
        StackInputKind::StringList => {
            let values = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| serde_json::Value::String(item.to_string()))
                .collect::<Vec<_>>();
            Ok(serde_json::Value::Array(values))
        }
    }
}

fn validate_string_stack_input(input: &StackInputDefinition, value: &str) -> Result<()> {
    if let Some(validation) = &input.validation {
        if let Some(values) = &validation.values {
            if !values.iter().any(|candidate| candidate == value) {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be one of: {}.", input.label, values.join(", ")),
                }));
            }
        }
        if let Some(min) = validation.min_length {
            if value.len() < min as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too short.", input.label),
                }));
            }
        }
        if let Some(max) = validation.max_length {
            if value.len() > max as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too long.", input.label),
                }));
            }
        }
    }
    Ok(())
}

fn can_prompt() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn prompt_input_value(input: &StackInputDefinition) -> Result<String> {
    let mut stderr = std::io::stderr();
    let prompt = if matches!(input.kind, StackInputKind::Secret) {
        format!("{} (secret): ", input.label)
    } else if let Some(placeholder) = input.placeholder.as_deref() {
        format!("{} [{}]: ", input.label, placeholder)
    } else {
        format!("{}: ", input.label)
    };
    stderr
        .write_all(prompt.as_bytes())
        .and_then(|_| stderr.flush())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write input prompt".to_string(),
        })?;

    let value = if matches!(input.kind, StackInputKind::Secret) {
        read_secret_line()?
    } else {
        let mut value = String::new();
        std::io::stdin()
            .read_line(&mut value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read input value".to_string(),
            })?;
        value
    };
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.is_empty() {
        if let Some(placeholder) = input.placeholder.as_deref() {
            return Ok(placeholder.to_string());
        }
    }
    Ok(value)
}

#[cfg(unix)]
fn read_secret_line() -> Result<String> {
    use std::os::fd::AsRawFd;

    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    let original = unsafe {
        if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
            return read_line_with_echo();
        }
        termios.assume_init()
    };
    let mut hidden = original;
    hidden.c_lflag &= !libc::ECHO;
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &hidden);
    }

    let result = read_line_with_echo();
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &original);
    }
    eprintln!();
    result
}

#[cfg(not(unix))]
fn read_secret_line() -> Result<String> {
    read_line_with_echo()
}

fn read_line_with_echo() -> Result<String> {
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to read input value".to_string(),
        })?;
    Ok(value)
}
