# KeysRegistry

## Example Usage

```typescript
import { KeysRegistry } from "@alienplatform/platform-api/models";

let value: KeysRegistry = {
  repositories: 228767,
  credentials: 91556,
  credentialPolicy: "mixed",
  lastVerifiedAt: new Date("2024-06-04T15:08:03.591Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `repositories`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentials`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialPolicy`                                                                            | [models.KeysCredentialPolicy](../models/keyscredentialpolicy.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lastVerifiedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |