# CurrentReleaseExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseExtendGcpStack } from "@alienplatform/platform-api/models";

let value: CurrentReleaseExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `condition`                                 | *models.CurrentReleaseExtendConditionUnion* | :heavy_minus_sign:                          | N/A                                         |
| `scope`                                     | *string*                                    | :heavy_check_mark:                          | Scope (project/resource level)              |