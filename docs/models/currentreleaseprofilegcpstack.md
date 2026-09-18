# CurrentReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: CurrentReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `condition`                                  | *models.CurrentReleaseProfileConditionUnion* | :heavy_minus_sign:                           | N/A                                          |
| `scope`                                      | *string*                                     | :heavy_check_mark:                           | Scope (project/resource level)               |