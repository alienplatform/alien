# ContainerRegistryStateRepository

## Example Usage

```typescript
import { ContainerRegistryStateRepository } from "@alienplatform/platform-api/models";

let value: ContainerRegistryStateRepository = {
  id: "crrepo_625temdq3bnu25jw9rcux",
  logicalName: "<value>",
  desiredState: "deleteRequested",
  status: "creating",
  verifiedAt: new Date("2026-06-17T16:21:20.305Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the container registry repository.                                      | crrepo_625temdq3bnu25jw9rcux                                                                  |
| `logicalName`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `desiredState`                                                                                | [models.ContainerRegistryStateDesiredState](../models/containerregistrystatedesiredstate.md)  | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `status`                                                                                      | [models.RepositoryStatus](../models/repositorystatus.md)                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `verifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |