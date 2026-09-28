# ConfigureProjectSourceRegistry

## Example Usage

```typescript
import { ConfigureProjectSourceRegistry } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceRegistry = {
  enabled: true,
  repositories: [
    "<value 1>",
    "<value 2>",
  ],
  credentialPolicy: "push-and-pull",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                              | *true*                                                                                                                 | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `repositories`                                                                                                         | *string*[]                                                                                                             | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `credentialPolicy`                                                                                                     | [operations.ConfigureProjectSourceCredentialPolicy](../../models/operations/configureprojectsourcecredentialpolicy.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |