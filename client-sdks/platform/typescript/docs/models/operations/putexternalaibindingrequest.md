# PutExternalAIBindingRequest

## Example Usage

```typescript
import { PutExternalAIBindingRequest } from "@alienplatform/platform-api/models/operations";

let value: PutExternalAIBindingRequest = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  putExternalAIBindingRequest: {
    provider: "databricks",
    workspaceUrl: "https://dead-morning.biz/",
    clientId: "<id>",
    clientSecret: "<value>",
    acknowledgeAlienCredentialAccess: true,
  },
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 | Example                                     |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `id`                                        | *string*                                    | :heavy_check_mark:                          | Unique identifier for the deployment group. | dg_r27ict8c7vcgsumpj90ackf7b                |
| `putExternalAIBindingRequest`               | *models.PutExternalAIBindingRequestUnion*   | :heavy_check_mark:                          | N/A                                         |                                             |