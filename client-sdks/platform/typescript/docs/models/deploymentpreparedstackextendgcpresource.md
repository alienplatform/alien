# DeploymentPreparedStackExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackExtendGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `condition`                                                  | *models.DeploymentPreparedStackExtendResourceConditionUnion* | :heavy_minus_sign:                                           | N/A                                                          |
| `scope`                                                      | *string*                                                     | :heavy_check_mark:                                           | Scope (project/resource level)                               |
