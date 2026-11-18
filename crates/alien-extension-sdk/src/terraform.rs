//! The Terraform module modifier, which edits Alien's own `.tf` files in place.
//!
//! The files are parsed with hcl-edit, which prints every byte it was not asked to change back
//! as it read it. The helpers here write new attributes and blocks with the whitespace
//! `terraform fmt` expects, so a modified module still passes `terraform fmt -check`.

use alien_core::hooks::{TemplateFiles, TerraformModifierOutput};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use hcl::edit::expr::Expression;
use hcl::edit::parser::parse_body;
use hcl::edit::structure::{Attribute, Block, Body, Structure};
use hcl::edit::{Decorate, Ident};
use indexmap::IndexMap;

/// Changes the Terraform module Alien generates for initial setup.
pub trait TerraformModifier {
    fn modify(&self, module: TfModule) -> Result<TfModule>;
}

/// Alien's Terraform module, file by file, and the file the extension adds its own resources to.
pub struct TfModule {
    extension: String,
    files: Vec<TfFile>,
}

struct TfFile {
    path: String,
    original: String,
    /// `None` for a file that is not Terraform, such as the module's README.
    body: Option<Body>,
}

impl TfModule {
    /// Reads a module's files for the extension named `extension`. Every `.tf` file must parse
    /// as HCL.
    pub fn new(extension: &str, files: IndexMap<String, String>) -> Result<Self> {
        let files = files
            .into_iter()
            .map(|(path, original)| {
                let body = if path.ends_with(".tf") {
                    Some(parse_body(&original).map_err(|error| {
                        AlienError::new(ErrorData::GenericError {
                            message: format!("{path} is not valid HCL: {error}"),
                        })
                    })?)
                } else {
                    None
                };
                Ok(TfFile {
                    path,
                    original,
                    body,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            extension: extension.to_string(),
            files,
        })
    }

    /// Adds an empty `resource "<resource_type>" "<name>"` block to `<extension>.tf`, the one file
    /// a modifier writes besides Alien's, and returns it. An earlier modifier of the same
    /// extension may have started the file; Alien refuses a module that has it before the
    /// extension's first modifier runs.
    pub fn add_resource(&mut self, resource_type: &str, name: &str) -> Result<&mut TfBlock> {
        self.add_to_extension_file("resource", resource_type, name)
    }

    /// Adds an empty `data "<data_type>" "<name>"` block to `<extension>.tf`, as
    /// [`add_resource`](Self::add_resource) adds a resource, and returns it. A modifier reads a
    /// value Alien's blocks don't carry through it, such as the Azure tenant from
    /// `azurerm_client_config`.
    pub fn add_data_source(&mut self, data_type: &str, name: &str) -> Result<&mut TfBlock> {
        self.add_to_extension_file("data", data_type, name)
    }

    fn add_to_extension_file(
        &mut self,
        identifier: &str,
        block_type: &str,
        name: &str,
    ) -> Result<&mut TfBlock> {
        let path = format!("{}.tf", self.extension);
        let index = match self.files.iter().position(|file| file.path == path) {
            Some(index) => index,
            None => {
                self.files.push(TfFile {
                    path,
                    original: String::new(),
                    body: Some(Body::new()),
                });
                self.files.len() - 1
            }
        };
        let body = self.files[index]
            .body
            .as_mut()
            .expect("the extension's file is Terraform");
        let mut block = Block::new(Ident::new(identifier));
        block.labels = vec![block_type.into(), name.into()];
        if !body.is_empty() {
            // A blank line between blocks, as `terraform fmt` leaves Alien's.
            block.decor_mut().set_prefix("\n");
        }
        body.push(block);
        let added = body.blocks_mut().last().expect("the block was just pushed");
        Ok(TfBlock::wrap(added))
    }

    /// Every `resource` block of a Terraform type, such as `aws_rds_cluster`, in every file.
    pub fn resources_of_type_mut<'a>(
        &'a mut self,
        resource_type: &'a str,
    ) -> impl Iterator<Item = &'a mut TfBlock> + 'a {
        self.files
            .iter_mut()
            .filter_map(|file| file.body.as_mut())
            .flat_map(|body| body.blocks_mut())
            .filter(move |block| {
                block.has_ident("resource")
                    && block
                        .labels
                        .first()
                        .is_some_and(|label| label.as_str() == resource_type)
            })
            .map(TfBlock::wrap)
    }

    /// The files whose text changed, by path.
    pub fn changed_files(&self) -> IndexMap<String, String> {
        self.files
            .iter()
            .filter_map(|file| {
                let text = file.body.as_ref()?.to_string();
                (text != file.original).then(|| (file.path.clone(), text))
            })
            .collect()
    }
}

/// A block in a [`TfModule`]: a resource, or a block nested in one.
///
/// A modifier adds attributes and blocks, and changes values. It can't remove an attribute
/// Alien emitted, such as a Cloud SQL instance's `tier`, which the provider may require:
///
/// ```
/// # use alien_extension_sdk::prelude::*;
/// fn larger(instance: &mut TfBlock) -> Result<()> {
///     instance.block_mut("settings")?.set_attribute("tier", "db-custom-2-7680");
///     Ok(())
/// }
/// ```
///
/// ```compile_fail
/// # use alien_extension_sdk::prelude::*;
/// fn without_tier(instance: &mut TfBlock) -> Result<()> {
///     instance.block_mut("settings")?.remove_attribute("tier");
///     Ok(())
/// }
/// ```
#[repr(transparent)]
pub struct TfBlock(Block);

impl TfBlock {
    fn wrap(block: &mut Block) -> &mut TfBlock {
        // SAFETY: `TfBlock` is a `repr(transparent)` wrapper around `Block`.
        unsafe { &mut *(block as *mut Block as *mut TfBlock) }
    }

    /// Sets an attribute, or adds it at the end of the block.
    ///
    /// A replaced value keeps the spacing around the old one. An added attribute joins the run
    /// of one-line attributes it follows, with its `=` aligned the way `terraform fmt` aligns
    /// them; the run's other lines change only when the new name is the longest.
    pub fn set_attribute(&mut self, name: &str, value: impl Into<Expression>) -> &mut Self {
        let mut value = value.into();
        let body = &mut self.0.body;
        if let Some(mut attribute) = body.get_attribute_mut(name) {
            let old = attribute.value_mut();
            *value.decor_mut() = old.decor().clone();
            *old = value;
            return self;
        }
        let mut attribute = Attribute::new(Ident::new(name), value);
        attribute.decor_mut().set_prefix(child_indent(body));
        let run = aligned_run(body);
        let width = body
            .iter()
            .skip(run.start)
            .filter_map(Structure::as_attribute)
            .map(|attribute| attribute.key.as_str().len())
            .chain([name.len()])
            .max()
            .unwrap_or(name.len());
        for index in run {
            if let Some(Structure::Attribute(attribute)) = body.get_mut(index) {
                let padding = width - attribute.key.as_str().len() + 1;
                attribute.key.decor_mut().set_suffix(" ".repeat(padding));
            }
        }
        attribute
            .key
            .decor_mut()
            .set_suffix(" ".repeat(width - name.len() + 1));
        body.push(attribute);
        self
    }

    /// The first nested block named `name` that Alien emitted, such as a Cloud SQL instance's
    /// `settings`. Fails when there is none.
    pub fn block_mut(&mut self, name: &str) -> Result<&mut TfBlock> {
        let header = self.header();
        self.0
            .body
            .blocks_mut()
            .find(|block| block.has_ident(name))
            .map(TfBlock::wrap)
            .ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!("{header} has no `{name}` block"),
                })
            })
    }

    /// Adds an empty nested block named `name` at the end of this one and returns it.
    pub fn add_block(&mut self, name: &str) -> &mut TfBlock {
        let indent = child_indent(&self.0.body);
        let mut block = Block::new(Ident::new(name));
        block.body.decor_mut().set_suffix(indent.clone());
        block.decor_mut().set_prefix(indent);
        self.0.body.push(block);
        let added = self
            .0
            .body
            .blocks_mut()
            .last()
            .expect("the block was just pushed");
        TfBlock::wrap(added)
    }

    /// The block's identifier and labels, as its first line shows them.
    fn header(&self) -> String {
        std::iter::once(self.0.ident.to_string())
            .chain(
                self.0
                    .labels
                    .iter()
                    .map(|label| format!("{:?}", label.as_str())),
            )
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The one-line attributes at the end of `body` that `terraform fmt` aligns with a line added
/// after them. A block, a value over several lines, a blank line or a comment ends the run.
fn aligned_run(body: &Body) -> std::ops::Range<usize> {
    let mut start = body.len();
    while start > 0 {
        let Some(Structure::Attribute(attribute)) = body.get(start - 1) else {
            break;
        };
        if attribute.value.to_string().contains('\n') {
            break;
        }
        start -= 1;
        let prefix = attribute
            .decor()
            .prefix()
            .map(|prefix| &**prefix)
            .unwrap_or_default();
        if prefix.contains('\n') || prefix.contains('#') {
            break;
        }
    }
    start..body.len()
}

/// The indentation of a block's contents: two spaces past its closing brace, which is the
/// whitespace the body keeps after its last line.
fn child_indent(body: &Body) -> String {
    let closing = body
        .decor()
        .suffix()
        .map(|suffix| suffix.rsplit('\n').next().unwrap_or_default().to_string())
        .unwrap_or_default();
    format!("{closing}  ")
}

pub(crate) fn run(
    extension: &str,
    modifier: &dyn TerraformModifier,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    let input: TemplateFiles = crate::read_input(input, "Terraform module")?;
    let module =
        TfModule::new(extension, input.files).map_err(alien_core::hooks::HookError::from)?;
    let module = modifier
        .modify(module)
        .map_err(alien_core::hooks::HookError::from)?;
    crate::write_output(&TerraformModifierOutput {
        changed: module.changed_files(),
    })
}
