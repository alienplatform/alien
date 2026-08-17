# CapabilityMaterialization

## Example Usage

```typescript
import { CapabilityMaterialization } from "@alienplatform/platform-api/models";

let value: CapabilityMaterialization = {
  projectCapabilities: {
    schemaVersion: 9196.58,
    capabilities: {},
  },
  source: {
    definitionId: "customer-key",
    definitionVersion: "<value>",
    releaseId: "<id>",
  },
  packages: [
    {
      type: "cloudformation",
      status: "failed",
    },
  ],
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `projectCapabilities`                                                                      | [models.ProjectCapabilities](../models/projectcapabilities.md)                             | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `source`                                                                                   | [models.CapabilityMaterializationSource](../models/capabilitymaterializationsource.md)     | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `packages`                                                                                 | [models.CapabilityMaterializationPackage](../models/capabilitymaterializationpackage.md)[] | :heavy_check_mark:                                                                         | N/A                                                                                        |