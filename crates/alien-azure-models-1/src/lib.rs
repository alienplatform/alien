//! Internal Azure model shard, balanced by generated line count for parallel compilation.

pub mod public_ip_address {
    include!(concat!(env!("OUT_DIR"), "/public_ip_address.rs"));
}
pub mod network_security_group {
    include!(concat!(env!("OUT_DIR"), "/network_security_group.rs"));
}
pub mod keyvault {
    include!(concat!(env!("OUT_DIR"), "/keyvault.rs"));
}
pub mod certificates {
    include!(concat!(env!("OUT_DIR"), "/certificates.rs"));
}
pub mod table {
    include!(concat!(env!("OUT_DIR"), "/table.rs"));
}
