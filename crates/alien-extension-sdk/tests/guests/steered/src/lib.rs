//! The `steered` extension: one modifier of each kind, each returning what its input asks for.
//! A CloudFormation template whose description starts with `STEER` returns the rest of the
//! description as the template; a Terraform module or Helm chart with a file `steer.json`
//! returns that file's contents as its output. Anything else returns no change.
//!
//! It implements the hooks ABI itself, since the SDK's modifier types can't return everything
//! a module could.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use alien_extension_sdk::prelude::*;
use std::collections::BTreeMap;

struct Unchanged;

impl CloudFormationModifier for Unchanged {
    fn modify(&self, template: CfTemplate) -> Result<CfTemplate> {
        Ok(template)
    }
}

impl TerraformModifier for Unchanged {
    fn modify(&self, module: TfModule) -> Result<TfModule> {
        Ok(module)
    }
}

impl HelmModifier for Unchanged {
    fn templates(&self, _chart: &HelmChart) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }
}

fn register(ext: &mut Extension) {
    ext.cloudformation_modifier(Unchanged);
    ext.terraform_modifier(Unchanged);
    ext.helm_modifier(Unchanged);
}

fn extension() -> Extension {
    Extension::new("steered", register)
}

/// The output the input asks the hook named `hook` for.
fn respond(hook: &str, input: &[u8]) -> Vec<u8> {
    let value: serde_json::Value = serde_json::from_slice(input).unwrap_or_default();
    if hook.starts_with("cloudformationModifier") {
        return match value["Description"]
            .as_str()
            .and_then(|description| description.strip_prefix("STEER"))
        {
            Some(template) => template.as_bytes().to_vec(),
            None => input.to_vec(),
        };
    }
    if let Some(output) = value["files"]["steer.json"].as_str() {
        return output.as_bytes().to_vec();
    }
    if hook.starts_with("terraformModifier") {
        b"{\"changed\":{}}".to_vec()
    } else {
        b"{\"templates\":{}}".to_vec()
    }
}

fn pack(output: Vec<u8>) -> i64 {
    let packed = output.as_ptr() as u32 as u64 | (output.len() as u64) << 32;
    std::mem::forget(output);
    packed as i64
}

#[cfg(target_arch = "wasm32")]
const _: () = {
    #[unsafe(no_mangle)]
    pub extern "C" fn alien_hooks_abi_v1() {}

    #[unsafe(no_mangle)]
    pub extern "C" fn alien_alloc(len: i32) -> i32 {
        alien_extension_sdk::abi::alloc(len)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn alien_hooks_manifest() -> i64 {
        alien_extension_sdk::abi::manifest(extension)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn alien_hooks_call(
        name_ptr: i32,
        name_len: i32,
        input_ptr: i32,
        input_len: i32,
    ) -> i64 {
        // SAFETY: the host wrote `name_len` and `input_len` bytes into buffers from `alien_alloc`.
        let (name, input) = unsafe {
            (
                Vec::from_raw_parts(name_ptr as *mut u8, name_len as usize, name_len as usize),
                Vec::from_raw_parts(input_ptr as *mut u8, input_len as usize, input_len as usize),
            )
        };
        pack(respond(&String::from_utf8_lossy(&name), &input))
    }
};
