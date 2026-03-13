# DataBuildingResource

## Example Usage

```typescript
import { DataBuildingResource } from "@aliendotdev/platform-api/models";

let value: DataBuildingResource = {
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
| `resourceType`                                                       | *string*                                                             | :heavy_check_mark:                                                   | Type of the resource: "function", "container", "worker"              |
| `type`                                                               | *"BuildingResource"*                                                 | :heavy_check_mark:                                                   | N/A                                                                  |