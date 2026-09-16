# ProjectCapabilitiesRegistry

## Example Usage

```typescript
import { ProjectCapabilitiesRegistry } from "@alienplatform/platform-api/models";

let value: ProjectCapabilitiesRegistry = {
  enabled: false,
  repositories: [],
  credentialPolicy: "pull-only",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `enabled`                                                                                      | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `repositories`                                                                                 | *string*[]                                                                                     | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `credentialPolicy`                                                                             | [models.ProjectCapabilitiesCredentialPolicy](../models/projectcapabilitiescredentialpolicy.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |