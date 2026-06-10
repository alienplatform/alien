# PackageModules

Information about a single Terraform module package for one target.

## Example Usage

```typescript
import { PackageModules } from "@alienplatform/platform-api/models";

let value: PackageModules = {
  downloadUrl: "https://colorful-flame.info",
  filename: "example.file",
  shasum: "<value>",
  size: 511419,
  source: "<value>",
  target: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `downloadUrl`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | Download URL for the module archive                                        |
| `filename`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | Filename of the module archive                                             |
| `shasum`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | SHA256 checksum of the archive                                             |
| `size`                                                                     | *number*                                                                   | :heavy_check_mark:                                                         | Size of the archive in bytes                                               |
| `source`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Terraform module source (hostname/namespace/name/provider, without scheme) |
| `target`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Terraform module target (aws, gcp, azure, eks, gke, aks)                   |
| `variables`                                                                | *string*[]                                                                 | :heavy_minus_sign:                                                         | Terraform input variables exposed by this module.                          |