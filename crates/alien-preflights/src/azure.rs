//! Shared Azure Container Apps platform constants used by both
//! compile-time validation and deploy-time mutation.

/// Valid Azure Container Apps CPU/memory combinations.
/// Each pair is (cpu_cores, memory_mb). The ratio is always 1 vCPU : 2 GiB.
pub const AZURE_VALID_COMBOS: [(f64, u32); 8] = [
    (0.25, 512),
    (0.5, 1024),
    (0.75, 1536),
    (1.0, 2048),
    (1.25, 2560),
    (1.5, 3072),
    (1.75, 3584),
    (2.0, 4096),
];

/// Returns true if `memory_mb` is one of the valid Azure Container Apps values.
pub fn is_valid_memory(memory_mb: u32) -> bool {
    AZURE_VALID_COMBOS.iter().any(|(_, mem)| *mem == memory_mb)
}

/// Returns the nearest valid Azure memory value (rounded up), or None if above max.
pub fn nearest_valid_memory(memory_mb: u32) -> Option<u32> {
    AZURE_VALID_COMBOS
        .iter()
        .find(|(_, mem)| *mem >= memory_mb)
        .map(|(_, mem)| *mem)
}

/// The maximum valid Azure Container Apps memory value.
pub fn max_memory() -> u32 {
    AZURE_VALID_COMBOS.last().expect("AZURE_VALID_COMBOS is non-empty").1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_memory() {
        assert!(is_valid_memory(512));
        assert!(is_valid_memory(1024));
        assert!(is_valid_memory(4096));
        assert!(!is_valid_memory(256));
        assert!(!is_valid_memory(768));
        assert!(!is_valid_memory(5000));
    }

    #[test]
    fn test_nearest_valid_memory() {
        assert_eq!(nearest_valid_memory(256), Some(512));
        assert_eq!(nearest_valid_memory(512), Some(512));
        assert_eq!(nearest_valid_memory(600), Some(1024));
        assert_eq!(nearest_valid_memory(4096), Some(4096));
        assert_eq!(nearest_valid_memory(5000), None);
    }

    #[test]
    fn test_max_memory() {
        assert_eq!(max_memory(), 4096);
    }
}
