# GroupRegistryRegistry

## Example Usage

```typescript
import { GroupRegistryRegistry } from "@alienplatform/platform-api/models";

let value: GroupRegistryRegistry = {
  repositories: 810010,
  credentials: 442281,
  credentialPolicy: "mixed",
  lastVerifiedAt: new Date("2025-02-24T18:46:47.162Z"),
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `repositories`                                                                                             | *number*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `credentials`                                                                                              | *number*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `credentialPolicy`                                                                                         | [models.ProjectCapabilityOverviewCredentialPolicy](../models/projectcapabilityoverviewcredentialpolicy.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `lastVerifiedAt`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)              | :heavy_check_mark:                                                                                         | N/A                                                                                                        |