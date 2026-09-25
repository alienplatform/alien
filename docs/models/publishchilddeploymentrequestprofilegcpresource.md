# PublishChildDeploymentRequestProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PublishChildDeploymentRequestProfileGcpResource } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.PublishChildDeploymentRequestProfileResourceConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |