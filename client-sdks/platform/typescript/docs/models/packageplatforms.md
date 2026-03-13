# PackagePlatforms

Information about a single Terraform provider package for a specific platform

## Example Usage

```typescript
import { PackagePlatforms } from "@aliendotdev/platform-api/models";

let value: PackagePlatforms = {
  downloadUrl: "https://best-attraction.info",
  filename: "example.file",
  shasum: "<value>",
  shasumsSignatureUrl: "https://vast-airmail.info",
  shasumsUrl: "https://blaring-sundae.org/",
  size: 307781,
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