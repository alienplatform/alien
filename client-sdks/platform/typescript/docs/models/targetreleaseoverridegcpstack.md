# TargetReleaseOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleaseOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: TargetReleaseOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `condition`                                  | *models.TargetReleaseOverrideConditionUnion* | :heavy_minus_sign:                           | N/A                                          |
| `scope`                                      | *string*                                     | :heavy_check_mark:                           | Scope (project/resource level)               |