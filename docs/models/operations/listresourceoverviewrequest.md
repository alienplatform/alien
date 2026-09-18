# ListResourceOverviewRequest

## Example Usage

```typescript
import { ListResourceOverviewRequest } from "@alienplatform/platform-api/models/operations";

let value: ListResourceOverviewRequest = {
  area: "container",
  project: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `area`                                                                                     | [operations.ListResourceOverviewArea](../../models/operations/listresourceoverviewarea.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `project`                                                                                  | *string*                                                                                   | :heavy_check_mark:                                                                         | Filter by project ID or name.                                                              |
| `deploymentGroupId`                                                                        | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `deploymentId`                                                                             | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |