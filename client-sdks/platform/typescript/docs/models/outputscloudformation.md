# OutputsCloudformation

Outputs from a CloudFormation package build

## Example Usage

```typescript
import { OutputsCloudformation } from "@alienplatform/platform-api/models";

let value: OutputsCloudformation = {
  launchStackUrl: "https://weird-newsprint.info/",
  sha256: "<value>",
  size: 243114,
  templateUrl: "https://teeming-legging.biz/",
  type: "cloudformation",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `launchStackUrl`                                                           | *string*                                                                   | :heavy_check_mark:                                                         | AWS Console quick-launch URL                                               |
| `sha256`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | SHA256 checksum of the template                                            |
| `size`                                                                     | *number*                                                                   | :heavy_check_mark:                                                         | Template size in bytes                                                     |
| `templateUrl`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | S3 URL to the CloudFormation template                                      |
| `type`                                                                     | [models.OutputsTypeCloudformation](../models/outputstypecloudformation.md) | :heavy_check_mark:                                                         | N/A                                                                        |