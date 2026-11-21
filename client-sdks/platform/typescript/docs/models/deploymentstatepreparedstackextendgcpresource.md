# DeploymentStatePreparedStackExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackExtendGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `condition`                                                          | *models.DeploymentStatePreparedStackExtendGcpResourceConditionUnion* | :heavy_minus_sign:                                                   | N/A                                                                  |
| `scope`                                                              | *string*                                                             | :heavy_check_mark:                                                   | Scope (project/resource level)                                       |