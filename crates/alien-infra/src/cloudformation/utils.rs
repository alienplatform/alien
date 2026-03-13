use heck::ToUpperCamelCase;

// TODO: the sanitize_to_pascal_case function is extremely important becuase it translates between our resource names to AWS resource names.
// Need to be much clearer about its name and potentially add resource prefix.

pub fn sanitize_to_pascal_case(name: &str) -> String {
    name.to_upper_camel_case()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(
            sanitize_to_pascal_case("my-function-name"),
            "MyFunctionName"
        );
        assert_eq!(sanitize_to_pascal_case("my_role"), "MyRole");
        assert_eq!(sanitize_to_pascal_case("S3Bucket"), "S3Bucket");
        assert_eq!(sanitize_to_pascal_case("s3-bucket"), "S3Bucket");
    }
}
