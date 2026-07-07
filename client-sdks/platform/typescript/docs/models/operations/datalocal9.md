# DataLocal9

## Example Usage

```typescript
import { DataLocal9 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal9 = {
  path: "/usr/include",
  pathExists: false,
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `isDirectory`                                                                                 | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `modifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `path`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `pathExists`                                                                                  | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `readonly`                                                                                    | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `secretMetadataListed`                                                                        | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [operations.DataStatus41](../../models/operations/datastatus41.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"local"*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |