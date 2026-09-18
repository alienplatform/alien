# ListAccessRequestsRequest

## Example Usage

```typescript
import { ListAccessRequestsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsRequest = {
  project: "<value>",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         | Example                                                             |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `project`                                                           | *string*                                                            | :heavy_check_mark:                                                  | Filter by project ID or name.                                       |                                                                     |
| `status`                                                            | [models.AccessRequestStatus](../../models/accessrequeststatus.md)[] | :heavy_minus_sign:                                                  | Filter by status.                                                   |                                                                     |
| `deploymentId`                                                      | *string*                                                            | :heavy_minus_sign:                                                  | Filter by deployment ID.                                            | dep_0c29fq4a2yjb7kx3smwdgxlc                                        |
| `limit`                                                             | *number*                                                            | :heavy_minus_sign:                                                  | Maximum number of items to return per page                          |                                                                     |
| `cursor`                                                            | *string*                                                            | :heavy_minus_sign:                                                  | Cursor for pagination - omit for first page                         |                                                                     |