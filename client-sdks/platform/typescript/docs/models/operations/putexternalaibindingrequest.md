# PutExternalAIBindingRequest

## Example Usage

```typescript
import { PutExternalAIBindingRequest } from "@alienplatform/platform-api/models/operations";

let value: PutExternalAIBindingRequest = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  workspace: "my-workspace",
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

| Field                                                                                                                                                                                  | Type                                                                                                                                                                                   | Required                                                                                                                                                                               | Description                                                                                                                                                                            | Example                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                                                   | *string*                                                                                                                                                                               | :heavy_check_mark:                                                                                                                                                                     | Unique identifier for the deployment group.                                                                                                                                            | dg_r27ict8c7vcgsumpj90ackf7b                                                                                                                                                           |
| `workspace`                                                                                                                                                                            | *string*                                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                                     | Workspace name. Required for user/session/OAuth requests. Optional for API keys because API keys are workspace-scoped; if provided with an API key, it must match the key's workspace. | my-workspace                                                                                                                                                                           |
| `putExternalAIBindingRequest`                                                                                                                                                          | *models.PutExternalAIBindingRequestUnion*                                                                                                                                              | :heavy_check_mark:                                                                                                                                                                     | N/A                                                                                                                                                                                    |                                                                                                                                                                                        |