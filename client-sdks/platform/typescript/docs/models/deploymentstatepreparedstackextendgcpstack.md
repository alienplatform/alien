# DeploymentStatePreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `condition`                                                    | *models.DeploymentStatePreparedStackExtendStackConditionUnion* | :heavy_minus_sign:                                             | N/A                                                            |
| `scope`                                                        | *string*                                                       | :heavy_check_mark:                                             | Scope (project/resource level)                                 |