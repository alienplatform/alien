# GetContainerAttentionResponse

Deployments needing attention.

## Example Usage

```typescript
import { GetContainerAttentionResponse } from "@aliendotdev/platform-api/models/operations";

let value: GetContainerAttentionResponse = {
  deployments: [],
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `deployments`                                                                                              | [operations.GetContainerAttentionDeployment](../../models/operations/getcontainerattentiondeployment.md)[] | :heavy_check_mark:                                                                                         | Deployments with issues needing attention                                                                  |