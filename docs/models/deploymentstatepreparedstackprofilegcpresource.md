# DeploymentStatePreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `condition`                                                        | *models.DeploymentStatePreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                 | N/A                                                                |
| `scope`                                                            | *string*                                                           | :heavy_check_mark:                                                 | Scope (project/resource level)                                     |