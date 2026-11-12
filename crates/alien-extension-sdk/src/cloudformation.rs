//! The CloudFormation template modifier.

use alien_cloudformation::{CfExpression, CfResource, CfTemplate};
use alien_core::Result;

/// Changes the CloudFormation template Alien generates for initial setup. It can add resources
/// and add, change or remove properties; Alien refuses a template that loses or retypes one of
/// its resources.
pub trait CloudFormationModifier {
    fn modify(&self, template: CfTemplate) -> Result<CfTemplate>;
}

/// Finds resources in a [`CfTemplate`].
pub trait CfTemplateExt {
    /// Every resource of a CloudFormation type, such as `AWS::RDS::DBCluster`.
    fn resources_of_type_mut<'a>(
        &'a mut self,
        resource_type: &'a str,
    ) -> impl Iterator<Item = &'a mut CfResource> + 'a;
}

impl CfTemplateExt for CfTemplate {
    fn resources_of_type_mut<'a>(
        &'a mut self,
        resource_type: &'a str,
    ) -> impl Iterator<Item = &'a mut CfResource> + 'a {
        self.resources
            .values_mut()
            .filter(move |resource| resource.resource_type == resource_type)
    }
}

/// Changes a [`CfResource`].
pub trait CfResourceExt {
    /// Sets a property, or replaces its value when the resource has it.
    fn set_property(&mut self, name: &str, value: impl Into<CfExpression>);
}

impl CfResourceExt for CfResource {
    fn set_property(&mut self, name: &str, value: impl Into<CfExpression>) {
        self.properties.insert(name.to_string(), value.into());
    }
}

pub(crate) fn run(
    modifier: &dyn CloudFormationModifier,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    let template: CfTemplate = crate::read_input(input, "CloudFormation template")?;
    let template = modifier
        .modify(template)
        .map_err(alien_core::hooks::HookError::from)?;
    crate::write_output(&template)
}
