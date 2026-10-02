# ImportDeploymentStatusRequest

## Example Usage

```typescript
import { ImportDeploymentStatusRequest } from "@alienplatform/manager-api/models/operations";

let value: ImportDeploymentStatusRequest = {
  id: "<id>",
  statusReport: {
    state: {
      "key": "Indiana",
      "key1": "Virginia",
    },
  },
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `id`                                                | *string*                                            | :heavy_check_mark:                                  | Deployment ID                                       |
| `statusReport`                                      | [models.StatusReport](../../models/statusreport.md) | :heavy_check_mark:                                  | N/A                                                 |