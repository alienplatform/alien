# CreateContainerRegistryRepositoryResponse

Repository requested

## Example Usage

```typescript
import { CreateContainerRegistryRepositoryResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateContainerRegistryRepositoryResponse = {
  id: "crrepo_625temdq3bnu25jw9rcux",
  logicalName: "<value>",
  desiredState: "present",
  status: "pending",
  verifiedAt: null,
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          | Example                                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                 | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Unique identifier for the container registry repository.                                                                             | crrepo_625temdq3bnu25jw9rcux                                                                                                         |
| `logicalName`                                                                                                                        | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |                                                                                                                                      |
| `desiredState`                                                                                                                       | [operations.CreateContainerRegistryRepositoryDesiredState](../../models/operations/createcontainerregistryrepositorydesiredstate.md) | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |                                                                                                                                      |
| `status`                                                                                                                             | [operations.CreateContainerRegistryRepositoryStatus](../../models/operations/createcontainerregistryrepositorystatus.md)             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |                                                                                                                                      |
| `verifiedAt`                                                                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                        | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |                                                                                                                                      |