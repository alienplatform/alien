# SyncGitNamespacesRequest

## Example Usage

```typescript
import { SyncGitNamespacesRequest } from "@aliendotdev/platform-api/models/operations";

let value: SyncGitNamespacesRequest = {
  provider: "github",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `provider`                                                                                   | [operations.SyncGitNamespacesProvider](../../models/operations/syncgitnamespacesprovider.md) | :heavy_check_mark:                                                                           | Git provider to sync                                                                         |