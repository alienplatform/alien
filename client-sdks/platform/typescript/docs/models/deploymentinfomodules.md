# DeploymentInfoModules

Information about a single Terraform module package for one target.

## Example Usage

```typescript
import { DeploymentInfoModules } from "@alienplatform/platform-api/models";

let value: DeploymentInfoModules = {
  downloadUrl: "https://gigantic-pleasure.net/",
  filename: "example.file",
  shasum: "<value>",
  size: 933138,
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