# TargetReleaseOverrideAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleaseOverrideAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetReleaseOverrideAzureBinding = {};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `resource`                                                                                   | [models.TargetReleaseOverrideAzureResource](../models/targetreleaseoverrideazureresource.md) | :heavy_minus_sign:                                                                           | Azure-specific binding specification                                                         |
| `stack`                                                                                      | [models.TargetReleaseOverrideAzureStack](../models/targetreleaseoverrideazurestack.md)       | :heavy_minus_sign:                                                                           | Azure-specific binding specification                                                         |