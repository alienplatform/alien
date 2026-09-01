# RemoteSandboxRegistry

## Example Usage

```typescript
import { RemoteSandboxRegistry } from "@alienplatform/platform-api/models";

let value: RemoteSandboxRegistry = {
  repositories: 293228,
  credentials: 373713,
  credentialPolicy: "mixed",
  lastVerifiedAt: new Date("2025-08-31T15:45:19.095Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `repositories`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentials`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialPolicy`                                                                            | [models.RemoteSandboxCredentialPolicy](../models/remotesandboxcredentialpolicy.md)            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lastVerifiedAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |