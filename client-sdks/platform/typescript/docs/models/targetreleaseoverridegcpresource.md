# TargetReleaseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleaseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: TargetReleaseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `condition`                                          | *models.TargetReleaseOverrideResourceConditionUnion* | :heavy_minus_sign:                                   | N/A                                                  |
| `scope`                                              | *string*                                             | :heavy_check_mark:                                   | Scope (project/resource level)                       |