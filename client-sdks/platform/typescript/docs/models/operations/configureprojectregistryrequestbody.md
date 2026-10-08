# ConfigureProjectRegistryRequestBody

## Example Usage

```typescript
import { ConfigureProjectRegistryRequestBody } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectRegistryRequestBody = {
  repositories: [
    "<value 1>",
    "<value 2>",
  ],
  credentialPolicy: "push-and-pull",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `repositories`                                                                                                             | *string*[]                                                                                                                 | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |
| `credentialPolicy`                                                                                                         | [operations.ConfigureProjectRegistryCredentialPolicy](../../models/operations/configureprojectregistrycredentialpolicy.md) | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |