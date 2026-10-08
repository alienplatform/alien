# PackageTargets

Information about a single CloudFormation template package for one target.

## Example Usage

```typescript
import { PackageTargets } from "@alienplatform/platform-api/models";

let value: PackageTargets = {
  launchStackUrl: "https://agitated-eggplant.org",
  sha256: "<value>",
  size: 190057,
  stackPolicyUrl: "https://straight-earth.net",
  target: "<value>",
  templateUrl: "https://ordinary-scout.com/",
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