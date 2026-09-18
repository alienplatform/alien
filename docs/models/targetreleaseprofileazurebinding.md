# TargetReleaseProfileAzureBinding

Generic binding configuration for permissions

## Example Usage

```typescript
import { TargetReleaseProfileAzureBinding } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfileAzureBinding = {};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `resource`                                                                                 | [models.TargetReleaseProfileAzureResource](../models/targetreleaseprofileazureresource.md) | :heavy_minus_sign:                                                                         | Azure-specific binding specification                                                       |
| `stack`                                                                                    | [models.TargetReleaseProfileAzureStack](../models/targetreleaseprofileazurestack.md)       | :heavy_minus_sign:                                                                         | Azure-specific binding specification                                                       |