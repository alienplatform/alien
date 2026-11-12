//! The bundles `alien extensions build` wrote, loaded for a stack on this machine.
//!
//! Each generated module records where it sits when alien.ts imports it, and the bundle is in
//! the same extension folder: `<root>/generated/index.ts` beside `<root>/target/alien-extension/`.
//! That holds for an extension built here and for one installed from a package that carries
//! both. A stack pins each extension by the sha256 of its core zip. A bundle runs only when its
//! zip has that digest and its hooks module has the sha256 the zip's manifest lists.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use alien_core::{ExtensionBundleManifest, EXTENSION_BUNDLE_MANIFEST, EXTENSION_HOOKS_MODULE};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_hooks_runtime::Runtime;
use alien_infra::LoadedExtensions;
use sha2::{Digest, Sha256};

use super::BUNDLE_DIR;
use crate::config::{ExtensionModule, LoadedConfiguration};
use crate::error::{ErrorData, Result};

/// The hooks module of each extension the loaded stack lists, from the bundle beside the
/// generated module its configuration imported for it.
pub fn load_local_bundles(configuration: &LoadedConfiguration) -> Result<LoadedExtensions> {
    let LoadedConfiguration {
        stack,
        config_file,
        extension_modules: modules,
    } = configuration;
    let mut loaded = LoadedExtensions::default();
    if stack.extensions.is_empty() {
        return Ok(loaded);
    }
    let runtime = Runtime::new().context(ErrorData::ConfigurationError {
        message: "the sandbox that runs extension hooks did not start".to_string(),
    })?;
    for extension in &stack.extensions {
        let refused = |message: String| ErrorData::ExtensionBundleRefused {
            extension: extension.name.clone(),
            message,
        };
        let (module, root) = extension_root(&extension.name, modules, config_file)?;
        let dir = root.join(BUNDLE_DIR);
        let path = dir.join(format!("{}-core.zip", extension.name));
        let zip = match std::fs::read(&path) {
            Ok(zip) => zip,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(AlienError::new(refused(format!(
                    "{} imports it from {}, and {} has no {}-core.zip. Run `alien extensions build` in {}",
                    file_name(config_file),
                    module.display(),
                    dir.display(),
                    extension.name,
                    root.display()
                ))));
            }
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(refused(format!("{} could not be read", path.display())));
            }
        };

        let actual = hex::encode(Sha256::digest(&zip));
        let pinned = extension.digest.strip_prefix("sha256:");
        if !pinned.is_some_and(|pinned| pinned.eq_ignore_ascii_case(&actual)) {
            return Err(AlienError::new(refused(format!(
                "the stack pins {}, and {} is sha256:{actual}. Rebuild the extension with `alien extensions build`, which writes its bundle and the generated module the stack imports together",
                extension.digest,
                path.display()
            ))));
        }

        let mut archive = zip::ZipArchive::new(Cursor::new(zip.as_slice()))
            .into_alien_error()
            .context(refused(format!("{} is not a zip", path.display())))?;
        let mut entry = |name: &str| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            archive
                .by_name(name)
                .into_alien_error()
                .and_then(|mut file| file.read_to_end(&mut bytes).into_alien_error())
                .context(refused(format!(
                    "{} has no readable {name}",
                    path.display()
                )))?;
            Ok(bytes)
        };
        let manifest: ExtensionBundleManifest =
            serde_json::from_slice(&entry(EXTENSION_BUNDLE_MANIFEST)?)
                .into_alien_error()
                .context(refused(format!(
                    "the {EXTENSION_BUNDLE_MANIFEST} in {} is not a bundle manifest",
                    path.display()
                )))?;
        let wasm = entry(EXTENSION_HOOKS_MODULE)?;
        let module = runtime
            .load(&wasm, &manifest.hooks_sha256)
            .context(refused(format!(
                "the hooks module in {} did not load",
                path.display()
            )))?;
        loaded = loaded.with_hooks(extension.name.clone(), module);
    }
    Ok(loaded)
}

/// The generated module that registered as the extension `name` while `config_file` loaded,
/// and the extension folder it sits in.
fn extension_root(
    name: &str,
    modules: &[ExtensionModule],
    config_file: &Path,
) -> Result<(PathBuf, PathBuf)> {
    let refused = |message: String| {
        AlienError::new(ErrorData::ExtensionBundleRefused {
            extension: name.to_string(),
            message,
        })
    };
    let mut urls: Vec<&str> = modules
        .iter()
        .filter(|module| module.name == name)
        .map(|module| module.url.as_str())
        .collect();
    urls.sort_unstable();
    urls.dedup();
    let url = match urls.as_slice() {
        [url] => *url,
        [] => {
            let file = file_name(config_file);
            let message = if config_file.extension().is_some_and(|ext| ext == "json") {
                format!(
                    "the stack lists it, and {file} can't import the module `alien extensions build` generated for it, so its bundle can't be found. List the extension in alien.ts, imported from that module"
                )
            } else {
                format!(
                    "the stack lists it, and {file} imported no module `alien extensions build` generated for it, so its bundle can't be found. Import the extension from its generated/index.ts, and rebuild it with `alien extensions build` if that module was written by an older CLI"
                )
            };
            return Err(refused(message));
        }
        urls => {
            return Err(refused(format!(
                "{} imported its generated module from more than one place, so the bundle to load is ambiguous: {}",
                file_name(config_file),
                urls.join(", ")
            )))
        }
    };
    let module = url::Url::parse(url)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .ok_or_else(|| {
            refused(format!(
                "its generated module was loaded from {url}, which is not a file"
            ))
        })?;
    let root = module
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            refused(format!(
                "its generated module {} is not in an extension folder",
                module.display()
            ))
        })?
        .to_path_buf();
    Ok((module, root))
}

/// The configuration file's name, as the developer knows it: alien.ts, alien.js or alien.json.
fn file_name(config_file: &Path) -> String {
    config_file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| config_file.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // `import.meta.url` percent-encodes the module's path, so a folder whose name has a space,
    // a non-ASCII letter, a literal `%20` or a `#` must decode back to that folder.
    #[test]
    fn a_module_url_decodes_to_its_extension_folder() {
        let module = ExtensionModule {
            name: "backups".to_string(),
            url: "file:///tmp/my%20app%20%C3%BC%2520%20%23x/backups/generated/index.ts".to_string(),
        };
        let (module_path, root) =
            extension_root("backups", &[module], Path::new("alien.ts")).unwrap();
        assert_eq!(
            module_path,
            PathBuf::from("/tmp/my app ü%20 #x/backups/generated/index.ts")
        );
        assert_eq!(root, PathBuf::from("/tmp/my app ü%20 #x/backups"));
    }

    #[test]
    fn the_refusals_name_the_configuration_file() {
        let error = extension_root("backups", &[], Path::new("/app/alien.json")).unwrap_err();
        assert!(
            error.to_string().contains(
                "the stack lists it, and alien.json can't import the module `alien extensions build` generated for it"
            ),
            "{error}"
        );
        let error = extension_root("backups", &[], Path::new("/app/alien.ts")).unwrap_err();
        assert!(
            error.to_string().contains(
                "the stack lists it, and alien.ts imported no module `alien extensions build` generated for it, so its bundle can't be found. Import the extension from its generated/index.ts"
            ),
            "{error}"
        );
    }
}
