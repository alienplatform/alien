# CloudformationTargets

Information about a single CloudFormation template package for one target.

## Example Usage

```typescript
import { CloudformationTargets } from "@alienplatform/platform-api/models";

let value: CloudformationTargets = {
  launchStackUrl: "https://recent-conservative.com",
  sha256: "<value>",
  size: 745297,
  stackPolicyUrl: "https://variable-toothpick.biz",
  target: "<value>",
  templateUrl: "https://rubbery-pilot.name/",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `launchStackUrl`                          | *string*                                  | :heavy_check_mark:                        | AWS Console quick-launch URL              |
| `sha256`                                  | *string*                                  | :heavy_check_mark:                        | SHA256 checksum of the template           |
| `size`                                    | *number*                                  | :heavy_check_mark:                        | Template size in bytes                    |
| `stackPolicyUrl`                          | *string*                                  | :heavy_check_mark:                        | S3 URL to the CloudFormation stack policy |
| `target`                                  | *string*                                  | :heavy_check_mark:                        | CloudFormation target (aws, eks)          |
| `templateUrl`                             | *string*                                  | :heavy_check_mark:                        | S3 URL to the CloudFormation template     |