# EventListItemResponseDataBuildingResource

## Example Usage

```typescript
import { EventListItemResponseDataBuildingResource } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataBuildingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "BuildingResource",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `relatedResources`                                                   | *string*[]                                                           | :heavy_minus_sign:                                                   | All resource names sharing this build (for deduped container groups) |
| `resourceName`                                                       | *string*                                                             | :heavy_check_mark:                                                   | Name of the resource being built                                     |
| `resourceType`                                                       | *string*                                                             | :heavy_check_mark:                                                   | Type of the resource: "worker", "container"                          |
| `type`                                                               | *"BuildingResource"*                                                 | :heavy_check_mark:                                                   | N/A                                                                  |
