# CurrentReleaseOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `condition`                                   | *models.CurrentReleaseOverrideConditionUnion* | :heavy_minus_sign:                            | N/A                                           |
| `scope`                                       | *string*                                      | :heavy_check_mark:                            | Scope (project/resource level)                |