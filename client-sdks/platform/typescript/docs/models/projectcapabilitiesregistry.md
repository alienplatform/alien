# ProjectCapabilitiesRegistry

## Example Usage

```typescript
import { ProjectCapabilitiesRegistry } from "@alienplatform/platform-api/models";

let value: ProjectCapabilitiesRegistry = {
  enabled: true,
  repositories: [
    "<value 1>",
    "<value 2>",
  ],
  credentialPolicy: "pull-only",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `enabled`                                                                                      | *true*                                                                                         | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `repositories`                                                                                 | *string*[]                                                                                     | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `credentialPolicy`                                                                             | [models.ProjectCapabilitiesCredentialPolicy](../models/projectcapabilitiescredentialpolicy.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |