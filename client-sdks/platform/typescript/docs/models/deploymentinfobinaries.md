# DeploymentInfoBinaries

Information about a single binary artifact

## Example Usage

```typescript
import { DeploymentInfoBinaries } from "@alienplatform/platform-api/models";

let value: DeploymentInfoBinaries = {
  sha256: "<value>",
  size: 512340,
  url: "https://unsightly-icebreaker.name/",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `sha256`                    | *string*                    | :heavy_check_mark:          | SHA256 checksum             |
| `size`                      | *number*                    | :heavy_check_mark:          | File size in bytes          |
| `url`                       | *string*                    | :heavy_check_mark:          | Download URL for the binary |