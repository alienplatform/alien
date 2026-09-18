# DeploymentInfoPlatforms

Information about a single Terraform provider package for a specific platform

## Example Usage

```typescript
import { DeploymentInfoPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentInfoPlatforms = {
  downloadUrl: "https://kooky-bin.name/",
  filename: "example.file",
  shasum: "<value>",
  shasumsSignatureUrl: "https://glass-reconsideration.info",
  shasumsUrl: "https://well-made-coliseum.biz/",
  size: 553172,
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `downloadUrl`                     | *string*                          | :heavy_check_mark:                | Download URL for the provider zip |
| `filename`                        | *string*                          | :heavy_check_mark:                | Filename of the provider zip      |
| `shasum`                          | *string*                          | :heavy_check_mark:                | SHA256 checksum of the zip file   |
| `shasumsSignatureUrl`             | *string*                          | :heavy_check_mark:                | URL to the shasums signature file |
| `shasumsUrl`                      | *string*                          | :heavy_check_mark:                | URL to the shasums file           |
| `size`                            | *number*                          | :heavy_check_mark:                | Size of the zip file in bytes     |