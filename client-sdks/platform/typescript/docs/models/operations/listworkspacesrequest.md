# ListWorkspacesRequest

## Example Usage

```typescript
import { ListWorkspacesRequest } from "@alienplatform/platform-api/models/operations";

let value: ListWorkspacesRequest = {};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `search`                                    | *string*                                    | :heavy_minus_sign:                          | Search workspaces by name                   |
| `limit`                                     | *number*                                    | :heavy_minus_sign:                          | Maximum number of items to return per page  |
| `cursor`                                    | *string*                                    | :heavy_minus_sign:                          | Cursor for pagination - omit for first page |