# ActiveRelease

## Example Usage

```typescript
import { ActiveRelease } from "@alienplatform/platform-api/models";

let value: ActiveRelease = {
  id: "rel_WbhQgksrawSKIpEN0NAssHX9",
  version: "<value>",
  stack: {},
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            | Example                                                |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `id`                                                   | *string*                                               | :heavy_check_mark:                                     | Unique identifier for the release.                     | rel_WbhQgksrawSKIpEN0NAssHX9                           |
| `version`                                              | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |                                                        |
| `stack`                                                | [models.StackByPlatform](../models/stackbyplatform.md) | :heavy_check_mark:                                     | N/A                                                    |                                                        |