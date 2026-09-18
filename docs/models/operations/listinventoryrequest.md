# ListInventoryRequest

## Example Usage

```typescript
import { ListInventoryRequest } from "@alienplatform/platform-api/models/operations";

let value: ListInventoryRequest = {
  project: "<value>",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `project`                     | *string*                      | :heavy_check_mark:            | Filter by project ID or name. |
| `deploymentGroupId`           | *string*                      | :heavy_minus_sign:            | N/A                           |
| `deploymentId`                | *string*                      | :heavy_minus_sign:            | N/A                           |