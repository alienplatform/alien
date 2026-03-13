# ManagementReleaseInfo2

## Example Usage

```typescript
import { ManagementReleaseInfo2 } from "@aliendotdev/platform-api/models";

let value: ManagementReleaseInfo2 = {
  override: {
    "key": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.ReleaseInfoOverrideUnion*[]>                                                                               | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |