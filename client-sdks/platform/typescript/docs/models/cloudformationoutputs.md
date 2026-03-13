# CloudformationOutputs

Outputs from a CloudFormation package build

## Example Usage

```typescript
import { CloudformationOutputs } from "@aliendotdev/platform-api/models";

let value: CloudformationOutputs = {
  launchStackUrl: "https://well-made-cricket.net",
  sha256: "<value>",
  size: 888258,
  templateUrl: "https://colorful-godparent.info/",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `launchStackUrl`                      | *string*                              | :heavy_check_mark:                    | AWS Console quick-launch URL          |
| `sha256`                              | *string*                              | :heavy_check_mark:                    | SHA256 checksum of the template       |
| `size`                                | *number*                              | :heavy_check_mark:                    | Template size in bytes                |
| `templateUrl`                         | *string*                              | :heavy_check_mark:                    | S3 URL to the CloudFormation template |