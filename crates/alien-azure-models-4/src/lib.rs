//! Internal Azure model shard, balanced by generated line count for parallel compilation.

pub mod virtual_network {
    include!(concat!(env!("OUT_DIR"), "/virtual_network.rs"));
}
pub mod storage {
    include!(concat!(env!("OUT_DIR"), "/storage.rs"));
}
pub mod resources {
    include!(concat!(env!("OUT_DIR"), "/resources.rs"));
}
pub mod managed_identity {
    include!(concat!(env!("OUT_DIR"), "/managed_identity.rs"));
}
pub mod secrets {
    include!(concat!(env!("OUT_DIR"), "/secrets.rs"));
}
