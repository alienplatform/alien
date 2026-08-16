# ConfigureProjectSourceRegistry

## Example Usage

```typescript
import { ConfigureProjectSourceRegistry } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceRegistry = {
  enabled: false,
  repositories: [
    "<value 1>",
    "<value 2>",
  ],
  credentialPolicy: "pull-only",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                              | *boolean*                                                                                                              | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `repositories`                                                                                                         | *string*[]                                                                                                             | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `credentialPolicy`                                                                                                     | [operations.ConfigureProjectSourceCredentialPolicy](../../models/operations/configureprojectsourcecredentialpolicy.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |