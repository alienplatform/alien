# UpdateDeploymentDomainsRequestBody

## Example Usage

```typescript
import { UpdateDeploymentDomainsRequestBody } from "@alienplatform/platform-api/models/operations";

let value: UpdateDeploymentDomainsRequestBody = {
  resourceId: "<id>",
  endpoints: {
    "key": "<value>",
    "key1": "<value>",
  },
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `resourceId`             | *string*                 | :heavy_check_mark:       | N/A                      |
| `endpoints`              | Record<string, *string*> | :heavy_check_mark:       | N/A                      |