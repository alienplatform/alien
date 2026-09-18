# ListReleaseBranchesRequest

## Example Usage

```typescript
import { ListReleaseBranchesRequest } from "@alienplatform/platform-api/models/operations";

let value: ListReleaseBranchesRequest = {};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `project`                                           | *string*                                            | :heavy_minus_sign:                                  | Filter by project ID or name.                       |
| `search`                                            | *string*                                            | :heavy_minus_sign:                                  | Search branches by name (case-insensitive contains) |
| `limit`                                             | *number*                                            | :heavy_minus_sign:                                  | Maximum number of branches to return                |