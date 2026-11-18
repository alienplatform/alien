//! The private path a sandbox's agent takes to its persistent store on AWS: an S3 gateway
//! endpoint, and one connector security-group rule to the region's AWS-managed S3 prefix list.
//!
//! CloudFormation cannot read a gateway endpoint's prefix list (`AWS::EC2::VPCEndpoint` returns
//! no such attribute), so the prefix list of every supported region is checked in here. The
//! same id holds in every account of a region. A region missing from the table is refused when
//! the stack is planned; a live test checks every row against `DescribeManagedPrefixLists`.

/// `(region, prefix list id)` of the AWS-managed `com.amazonaws.<region>.s3` list.
pub const S3_PREFIX_LISTS: &[(&str, &str)] = &[
    ("ap-northeast-1", "pl-61a54008"),
    ("ap-northeast-2", "pl-78a54011"),
    ("ap-northeast-3", "pl-a4a540cd"),
    ("ap-south-1", "pl-78a54011"),
    ("ap-southeast-1", "pl-6fa54006"),
    ("ap-southeast-2", "pl-6ca54005"),
    ("ca-central-1", "pl-7da54014"),
    ("eu-central-1", "pl-6ea54007"),
    ("eu-north-1", "pl-c3aa4faa"),
    ("eu-west-1", "pl-6da54004"),
    ("eu-west-2", "pl-7ca54015"),
    ("eu-west-3", "pl-23ad484a"),
    ("sa-east-1", "pl-6aa54003"),
    ("us-east-1", "pl-63a5400a"),
    ("us-east-2", "pl-7ba54012"),
    ("us-west-1", "pl-6ba54002"),
    ("us-west-2", "pl-68a54001"),
];

/// The S3 prefix list of `region`, or `None` where persistent storage cannot be set up.
pub fn s3_prefix_list(region: &str) -> Option<&'static str> {
    S3_PREFIX_LISTS
        .iter()
        .find(|(name, _)| *name == region)
        .map(|(_, id)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_unique_and_well_formed() {
        let regions: Vec<&str> = S3_PREFIX_LISTS.iter().map(|(r, _)| *r).collect();
        let mut sorted = regions.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(regions, sorted);
        for (_, id) in S3_PREFIX_LISTS {
            assert!(id.starts_with("pl-") && id.len() == 11, "{id}");
        }
        assert_eq!(s3_prefix_list("us-east-2"), Some("pl-7ba54012"));
        assert_eq!(s3_prefix_list("mars-north-1"), None);
    }
}
