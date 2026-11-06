# CreateReleaseChannelRequestBody

## Example Usage

```typescript
import { CreateReleaseChannelRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateReleaseChannelRequestBody = {
  name: "<value>",
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        | Example                            |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `name`                             | *string*                           | :heavy_check_mark:                 | Channel the release was signed for |                                    |
| `releaseId`                        | *string*                           | :heavy_minus_sign:                 | Unique identifier for the release. | rel_WbhQgksrawSKIpEN0NAssHX9E2Su   |