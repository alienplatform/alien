# ModelsRegistry

## Example Usage

```typescript
import { ModelsRegistry } from "@alienplatform/platform-api/models";

let value: ModelsRegistry = {
  repositories: 921431,
  credentials: 717893,
  credentialPolicy: "pull-only",
  lastVerifiedAt: new Date("2026-09-11T20:57:03.415Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `repositories`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentials`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialPolicy`                                                                            | [models.ModelsCredentialPolicy](../models/modelscredentialpolicy.md)                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lastVerifiedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |