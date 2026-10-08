# TargetReleaseExtendAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleaseExtendAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetReleaseExtendAzureBinding = {};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `resource`                                                                               | [models.TargetReleaseExtendAzureResource](../models/targetreleaseextendazureresource.md) | :heavy_minus_sign:                                                                       | Azure-specific binding specification                                                     |
| `stack`                                                                                  | [models.TargetReleaseExtendAzureStack](../models/targetreleaseextendazurestack.md)       | :heavy_minus_sign:                                                                       | Azure-specific binding specification                                                     |