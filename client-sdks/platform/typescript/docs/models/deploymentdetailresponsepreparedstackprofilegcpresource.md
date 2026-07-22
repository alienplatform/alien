# DeploymentDetailResponsePreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `condition`                                                                 | *models.DeploymentDetailResponsePreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                          | N/A                                                                         |
| `scope`                                                                     | *string*                                                                    | :heavy_check_mark:                                                          | Scope (project/resource level)                                              |
