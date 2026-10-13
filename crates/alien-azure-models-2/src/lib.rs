//! Internal Azure model shard, balanced by generated line count for parallel compilation.

pub mod compute_rp {
    include!(concat!(env!("OUT_DIR"), "/compute_rp.rs"));
}
pub mod disk_rp {
    include!(concat!(env!("OUT_DIR"), "/disk_rp.rs"));
}
pub mod blob {
    include!(concat!(env!("OUT_DIR"), "/blob.rs"));
}
pub mod managed_environments_dapr_components {
    include!(concat!(
        env!("OUT_DIR"),
        "/managed_environments_dapr_components.rs"
    ));
}
pub mod authorization_role_assignments {
    include!(concat!(
        env!("OUT_DIR"),
        "/authorization_role_assignments.rs"
    ));
}

#[cfg(test)]
mod tests {
    use super::compute_rp::VirtualMachineScaleSetVmListResult;
    use serde_json::json;

    #[test]
    fn vmss_inventory_accepts_sizes_outside_the_schema_examples() {
        for size in ["Standard_D2s_v4", "Standard_FutureSku"] {
            let response = json!({
                "value": [{
                    "location": "eastus",
                    "instanceId": "0",
                    "properties": { "hardwareProfile": { "vmSize": size } }
                }]
            });
            let inventory: VirtualMachineScaleSetVmListResult =
                serde_json::from_value(response).expect("extensible VM sizes must deserialize");
            assert_eq!(inventory.value.len(), 1);
            assert_eq!(inventory.value[0].instance_id.as_deref(), Some("0"));
            let encoded = serde_json::to_value(&inventory).unwrap();
            assert_eq!(
                encoded["value"][0]["properties"]["hardwareProfile"]["vmSize"],
                size
            );
        }
    }
}
