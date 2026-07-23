# DeploymentPendingPreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `condition`                                                          | *models.DeploymentPendingPreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                   | N/A                                                                  |
| `scope`                                                              | *string*                                                             | :heavy_check_mark:                                                   | Scope (project/resource level)                                       |
