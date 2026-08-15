# ProjectListItemResponseRegistry

## Example Usage

```typescript
import { ProjectListItemResponseRegistry } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseRegistry = {
  enabled: true,
  repositories: [
    "<value 1>",
    "<value 2>",
  ],
  credentialPolicy: "pull-only",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `enabled`                                                                                              | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `repositories`                                                                                         | *string*[]                                                                                             | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `credentialPolicy`                                                                                     | [models.ProjectListItemResponseCredentialPolicy](../models/projectlistitemresponsecredentialpolicy.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |