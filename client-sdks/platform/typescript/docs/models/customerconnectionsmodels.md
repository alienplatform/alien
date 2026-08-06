# CustomerConnectionsModels

## Example Usage

```typescript
import { CustomerConnectionsModels } from "@alienplatform/platform-api/models";

let value: CustomerConnectionsModels = {
  enabled: false,
  allowedProviders: [
    "anthropic",
  ],
  requirements: [],
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `enabled`                                                                                      | *boolean*                                                                                      | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `allowedProviders`                                                                             | [models.CustomerConnectionsAllowedProvider](../models/customerconnectionsallowedprovider.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `requirements`                                                                                 | [models.CustomerConnectionsRequirement](../models/customerconnectionsrequirement.md)[]         | :heavy_check_mark:                                                                             | N/A                                                                                            |