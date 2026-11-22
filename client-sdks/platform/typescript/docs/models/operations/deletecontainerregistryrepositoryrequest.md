# DeleteContainerRegistryRepositoryRequest

## Example Usage

```typescript
import { DeleteContainerRegistryRepositoryRequest } from "@alienplatform/platform-api/models/operations";

let value: DeleteContainerRegistryRepositoryRequest = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  repositoryId: "crrepo_625temdq3bnu25jw9rcux",
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        | Example                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                               | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Unique identifier for the deployment group.                                                                                        | dg_r27ict8c7vcgsumpj90ackf7b                                                                                                       |
| `repositoryId`                                                                                                                     | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Unique identifier for the container registry repository.                                                                           | crrepo_625temdq3bnu25jw9rcux                                                                                                       |
| `requestBody`                                                                                                                      | [operations.DeleteContainerRegistryRepositoryRequestBody](../../models/operations/deletecontainerregistryrepositoryrequestbody.md) | :heavy_minus_sign:                                                                                                                 | N/A                                                                                                                                |                                                                                                                                    |