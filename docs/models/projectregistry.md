# ProjectRegistry

## Example Usage

```typescript
import { ProjectRegistry } from "@alienplatform/platform-api/models";

let value: ProjectRegistry = {
  enabled: true,
  repositories: [
    "<value 1>",
  ],
  credentialPolicy: "push-and-pull",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `enabled`                                                              | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `repositories`                                                         | *string*[]                                                             | :heavy_check_mark:                                                     | N/A                                                                    |
| `credentialPolicy`                                                     | [models.ProjectCredentialPolicy](../models/projectcredentialpolicy.md) | :heavy_check_mark:                                                     | N/A                                                                    |