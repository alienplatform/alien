# TargetReleaseManagement2

## Example Usage

```typescript
import { TargetReleaseManagement2 } from "@alienplatform/platform-api/models";

let value: TargetReleaseManagement2 = {
  override: {
    "key": [],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.TargetReleaseOverrideUnion*[]>                                                                             | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |