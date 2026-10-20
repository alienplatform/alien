# GetResourceDeploymentDetailRelease

The project release that ships this image for the resource, or null when none does.

## Example Usage

```typescript
import { GetResourceDeploymentDetailRelease } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailRelease = {
  id: "<id>",
  version: "<value>",
  relation: "desired",
};
```

## Fields

| Field                                                                                                                                                                       | Type                                                                                                                                                                        | Required                                                                                                                                                                    | Description                                                                                                                                                                 |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                                        | *string*                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                          | N/A                                                                                                                                                                         |
| `version`                                                                                                                                                                   | *string*                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                          | N/A                                                                                                                                                                         |
| `relation`                                                                                                                                                                  | [operations.Relation](../../models/operations/relation.md)                                                                                                                  | :heavy_check_mark:                                                                                                                                                          | `desired`: the release the deployment is updating to. `current`: the release it last installed. `other`: any other release of the project, such as one whose update failed. |