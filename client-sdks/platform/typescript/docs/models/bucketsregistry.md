# BucketsRegistry

## Example Usage

```typescript
import { BucketsRegistry } from "@alienplatform/platform-api/models";

let value: BucketsRegistry = {
  repositories: 345081,
  credentials: 161061,
  credentialPolicy: "none",
  lastVerifiedAt: new Date("2025-09-11T06:06:40.363Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `repositories`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentials`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialPolicy`                                                                            | [models.BucketsCredentialPolicy](../models/bucketscredentialpolicy.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lastVerifiedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |