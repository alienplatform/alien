# DeploymentStatePendingPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `condition`                                                            | *models.DeploymentStatePendingPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                     | N/A                                                                    |
| `scope`                                                                | *string*                                                               | :heavy_check_mark:                                                     | Scope (project/resource level)                                         |