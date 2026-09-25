# PublishChildDeploymentRequestOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PublishChildDeploymentRequestOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `condition`                                                  | *models.PublishChildDeploymentRequestOverrideConditionUnion* | :heavy_minus_sign:                                           | N/A                                                          |
| `scope`                                                      | *string*                                                     | :heavy_check_mark:                                           | Scope (project/resource level)                               |