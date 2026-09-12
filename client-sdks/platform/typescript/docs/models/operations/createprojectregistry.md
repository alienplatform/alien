# CreateProjectRegistry

## Example Usage

```typescript
import { CreateProjectRegistry } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectRegistry = {
  enabled: true,
  repositories: [
    "<value 1>",
  ],
  credentialPolicy: "push-and-pull",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                            | *true*                                                                                               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `repositories`                                                                                       | *string*[]                                                                                           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `credentialPolicy`                                                                                   | [operations.CreateProjectCredentialPolicy](../../models/operations/createprojectcredentialpolicy.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |