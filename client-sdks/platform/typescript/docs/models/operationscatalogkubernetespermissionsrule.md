# OperationsCatalogKubernetesPermissionsRule

## Example Usage

```typescript
import { OperationsCatalogKubernetesPermissionsRule } from "@alienplatform/platform-api/models";

let value: OperationsCatalogKubernetesPermissionsRule = {
  apiGroup: "<value>",
  resource: "<value>",
  verbs: [],
  reason: "<value>",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `apiGroup`                                                                                                     | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `resource`                                                                                                     | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `verbs`                                                                                                        | [models.OperationsCatalogKubernetesPermissionsVerb](../models/operationscatalogkubernetespermissionsverb.md)[] | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `resourceNames`                                                                                                | *string*[]                                                                                                     | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `reason`                                                                                                       | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |