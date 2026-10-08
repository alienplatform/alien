# DeploymentPreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `condition`                                                   | *models.DeploymentPreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                            | N/A                                                           |
| `scope`                                                       | *string*                                                      | :heavy_check_mark:                                            | Scope (project/resource level)                                |
