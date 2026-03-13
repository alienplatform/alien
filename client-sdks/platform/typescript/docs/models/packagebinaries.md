# PackageBinaries

Information about a single binary artifact

## Example Usage

```typescript
import { PackageBinaries } from "@aliendotdev/platform-api/models";

let value: PackageBinaries = {
  sha256: "<value>",
  size: 87451,
  url: "https://dependent-finer.biz",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `sha256`                    | *string*                    | :heavy_check_mark:          | SHA256 checksum             |
| `size`                      | *number*                    | :heavy_check_mark:          | File size in bytes          |
| `url`                       | *string*                    | :heavy_check_mark:          | Download URL for the binary |