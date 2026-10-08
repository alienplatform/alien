# CreateProjectFromTemplateRegistry

## Example Usage

```typescript
import { CreateProjectFromTemplateRegistry } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateRegistry = {
  enabled: true,
  repositories: [],
  credentialPolicy: "pull-only",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                                    | *true*                                                                                                                       | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `repositories`                                                                                                               | *string*[]                                                                                                                   | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `credentialPolicy`                                                                                                           | [operations.CreateProjectFromTemplateCredentialPolicy](../../models/operations/createprojectfromtemplatecredentialpolicy.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |