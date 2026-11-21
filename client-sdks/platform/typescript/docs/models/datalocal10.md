# DataLocal10

## Example Usage

```typescript
import { DataLocal10 } from "@alienplatform/platform-api/models";

let value: DataLocal10 = {
  path: "/home/user",
  pathExists: true,
  secretMetadataListed: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
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
| `status`                                                                                      | [models.ResourceHeartbeatStatus45](../models/resourceheartbeatstatus45.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"local"*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |