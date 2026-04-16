# ExtendReleaseInfoGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { ExtendReleaseInfoGcpStack } from "@alienplatform/platform-api/models";

let value: ExtendReleaseInfoGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `condition`                              | *models.ExtendReleaseInfoConditionUnion* | :heavy_minus_sign:                       | N/A                                      |
| `scope`                                  | *string*                                 | :heavy_check_mark:                       | Scope (project/resource level)           |