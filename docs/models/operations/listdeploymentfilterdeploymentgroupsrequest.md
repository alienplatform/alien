# ListDeploymentFilterDeploymentGroupsRequest

## Example Usage

```typescript
import { ListDeploymentFilterDeploymentGroupsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentFilterDeploymentGroupsRequest = {};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `project`                         | *string*                          | :heavy_minus_sign:                | Filter by project ID or name.     |
| `search`                          | *string*                          | :heavy_minus_sign:                | Search deployment groups by name  |
| `limit`                           | *number*                          | :heavy_minus_sign:                | Maximum number of items to return |