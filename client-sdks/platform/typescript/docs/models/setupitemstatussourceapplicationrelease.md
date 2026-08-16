# SetupItemStatusSourceApplicationRelease

## Example Usage

```typescript
import { SetupItemStatusSourceApplicationRelease } from "@alienplatform/platform-api/models";

let value: SetupItemStatusSourceApplicationRelease = {
  type: "application-release",
  releaseChannel: "<value>",
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        | Example                            |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `type`                             | *"application-release"*            | :heavy_check_mark:                 | N/A                                |                                    |
| `releaseChannel`                   | *string*                           | :heavy_check_mark:                 | N/A                                |                                    |
| `releaseId`                        | *string*                           | :heavy_check_mark:                 | Unique identifier for the release. | rel_WbhQgksrawSKIpEN0NAssHX9       |
| `resourceId`                       | *string*                           | :heavy_minus_sign:                 | N/A                                |                                    |