# CustomerConnectionMaterialization

## Example Usage

```typescript
import { CustomerConnectionMaterialization } from "@alienplatform/platform-api/models";

let value: CustomerConnectionMaterialization = {
  customerConnections: {
    schemaVersion: 7597.03,
    connections: {},
  },
  source: {
    definitionId: "customer-ai",
    definitionVersion: "<value>",
    releaseId: "<id>",
  },
  packages: [],
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `customerConnections`                                                                                      | [models.CustomerConnections](../models/customerconnections.md)                                             | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `source`                                                                                                   | [models.CustomerConnectionMaterializationSource](../models/customerconnectionmaterializationsource.md)     | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `packages`                                                                                                 | [models.CustomerConnectionMaterializationPackage](../models/customerconnectionmaterializationpackage.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |