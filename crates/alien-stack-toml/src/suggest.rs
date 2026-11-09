//! "Did you mean" hints for names that match no declared one.

use strsim::jaro_winkler;

const MIN_SIMILARITY: f64 = 0.8;

/// The candidate closest to `name`, if any is close enough to be a likely typo.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .into_iter()
        .map(|candidate| (candidate, jaro_winkler(name, candidate)))
        .filter(|(_, similarity)| *similarity >= MIN_SIMILARITY)
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(candidate, _)| candidate)
}

/// ` (did you mean 'x'?)` for the closest candidate, or nothing.
pub fn hint<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    closest(name, candidates)
        .map(|candidate| format!(" (did you mean '{candidate}'?)"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_the_closest_name() {
        assert_eq!(closest("wrkers", ["workers", "gpu"]), Some("workers"));
        assert_eq!(
            closest("job_worker", ["job-worker", "api"]),
            Some("job-worker")
        );
    }

    #[test]
    fn stays_quiet_when_nothing_is_close() {
        assert_eq!(closest("database", ["workers", "gpu"]), None);
        assert_eq!(hint("database", ["workers"]), "");
        assert_eq!(hint("wrkers", ["workers"]), " (did you mean 'workers'?)");
    }
}
