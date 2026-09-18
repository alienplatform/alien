# ObservedInventoryBatch

## Example Usage

```typescript
import { ObservedInventoryBatch } from "@alienplatform/platform-api/models";

let value: ObservedInventoryBatch = {
  backend: "kubernetes",
  complete: false,
  controllerPlatform: "machines",
  inventoryScope: "<value>",
  observedAt: new Date("2024-09-10T06:27:56.143Z"),
  resources: [
    {
      displayName: "Julianne_Moen56",
      health: "unhealthy",
      lifecycle: "creating",
      partial: false,
      providerKind: "<value>",
      providerStale: false,
      rawIdentity: "<value>",
    },
  ],
  sourceKind: "<value>",
};
```

## Fields

| Field                                                                                                                                                                    | Type                                                                                                                                                                     | Required                                                                                                                                                                 | Description                                                                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `backend`                                                                                                                                                                | [models.ObservedInventoryBatchBackend](../models/observedinventorybatchbackend.md)                                                                                       | :heavy_check_mark:                                                                                                                                                       | Backend whose observer produced this snapshot.                                                                                                                           |
| `complete`                                                                                                                                                               | *boolean*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                       | Whether this batch is a complete replacement for the scope. Complete<br/>batches tombstone previously observed rows in the same scope when they<br/>are absent from `resources`. |
| `controllerPlatform`                                                                                                                                                     | [models.ObservedInventoryBatchControllerPlatform](../models/observedinventorybatchcontrollerplatform.md)                                                                 | :heavy_check_mark:                                                                                                                                                       | Represents the target cloud platform.                                                                                                                                    |
| `inventoryScope`                                                                                                                                                         | *string*                                                                                                                                                                 | :heavy_check_mark:                                                                                                                                                       | Stable scope for the provider list operation that produced this batch.                                                                                                   |
| `observedAt`                                                                                                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                                            | :heavy_check_mark:                                                                                                                                                       | Time the inventory scope was observed.                                                                                                                                   |
| `resources`                                                                                                                                                              | [models.ObservedInventoryBatchResource](../models/observedinventorybatchresource.md)[]                                                                                   | :heavy_check_mark:                                                                                                                                                       | N/A                                                                                                                                                                      |
| `sourceKind`                                                                                                                                                             | *string*                                                                                                                                                                 | :heavy_check_mark:                                                                                                                                                       | Writer/source for this inventory pass, such as `operator` or<br/>`manager-observer`.                                                                                     |