# DeploymentStatePendingPreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `condition`                                                               | *models.DeploymentStatePendingPreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                        | N/A                                                                       |
| `scope`                                                                   | *string*                                                                  | :heavy_check_mark:                                                        | Scope (project/resource level)                                            |